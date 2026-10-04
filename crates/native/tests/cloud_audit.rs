#![cfg(feature = "cloud-audit")]
use backupduck_cloud_audit::{
    Error as AuditError, HashLookup, ItemInfo, LockedMove, Result as AuditResult,
};
use backupduck_core::*;
use backupduck_native::{
    backupduck_call, backupduck_free,
    cloud::{CloudLookup, LockOptions, LockReport, RunOptions, RunReport},
    ReceiverHost, SenderHost,
};
mod common;
use backupduck_store::retention::GalleryCopy;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQ: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "backupduck-cloud-audit-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Scripted Google library: SHA-1 -> (media key, takes up space). The dedup
/// key of `key` is `dedup-key`; a Locked Folder move removes the item unless
/// its SHA-1 is `sticky`.
#[derive(Default)]
struct Fake {
    library: HashMap<String, (String, Option<bool>)>,
    expire_lookup: bool,
    expire_info: bool,
    /// Expire every lookup once this many have been made.
    expire_after_lookups: Option<usize>,
    move_error: Option<fn() -> AuditError>,
    sticky: HashSet<String>,
    lookups: usize,
    moves: Vec<Vec<String>>,
}
impl CloudLookup for Fake {
    async fn lookup(&mut self, sha1_hex: &[String]) -> AuditResult<Vec<Option<HashLookup>>> {
        self.lookups += 1;
        if self.expire_lookup || self.expire_after_lookups.is_some_and(|n| self.lookups > n) {
            return Err(AuditError::SessionExpired);
        }
        Ok(sha1_hex
            .iter()
            .map(|h| {
                self.library.get(h).map(|(key, _)| HashLookup {
                    hash_b64: String::new(),
                    media_key: key.clone(),
                    dedup_key: Some(format!("dedup-{key}")),
                    device_model: Some("Pixel 9".into()),
                    width: None,
                    height: None,
                    timestamp_ms: None,
                    creation_timestamp_ms: None,
                })
            })
            .collect())
    }
    async fn info(&mut self, media_keys: &[String]) -> AuditResult<Vec<Option<ItemInfo>>> {
        if self.expire_info {
            return Err(AuditError::SessionExpired);
        }
        Ok(media_keys
            .iter()
            .map(|key| {
                let space = self.library.values().find(|(k, _)| k == key)?.1;
                Some(ItemInfo {
                    media_key: key.clone(),
                    file_name: None,
                    size: None,
                    takes_up_space: space,
                    space_taken: None,
                    is_original_quality: None,
                })
            })
            .collect())
    }
    async fn move_to_locked(&mut self, dedup_keys: &[String]) -> AuditResult<LockedMove> {
        if let Some(error) = self.move_error {
            return Err(error());
        }
        self.moves.push(dedup_keys.to_vec());
        let mut moved = LockedMove::default();
        let sticky = &self.sticky;
        self.library.retain(|sha1, (key, _)| {
            let hit = dedup_keys.contains(&format!("dedup-{key}"));
            if hit {
                moved.new_keys.push(format!("locked-{key}"));
                moved.removed_keys.push(key.clone());
            }
            !hit || sticky.contains(sha1)
        });
        Ok(moved)
    }
}

fn sha1(c: char) -> String {
    c.to_string().repeat(40)
}

/// Send one asset through the sender, then publish its gallery copy on the
/// receiver with an hour-old publication time so it is due for lookup.
async fn publish(t: &Temp, r: &ReceiverHost, sender: &SenderHost, name: &str, c: char) -> String {
    let bytes = name.as_bytes();
    let path = t.0.join(name);
    std::fs::write(&path, bytes).unwrap();
    let asset = Asset {
        version: PROTOCOL_VERSION,
        source_id: name.into(),
        revision: "1".into(),
        kind: AssetKind::Photo,
        metadata: BTreeMap::new(),
        resources: vec![Resource {
            role: ResourceRole::Photo,
            filename: format!("{name}.jpg"),
            media_type: "image/jpeg".into(),
            size: bytes.len() as u64,
            sha256: digest(bytes),
        }],
    };
    let id = asset.id().unwrap();
    sender
        .enqueue(
            &r.pairing.receiver_id,
            asset,
            BTreeMap::from([(digest(bytes), path.to_str().unwrap().to_string())]),
        )
        .unwrap();
    sender.run_once(&r.pairing).await.unwrap().unwrap();
    gallery(t, r, &id, c);
    id
}

fn gallery(t: &Temp, r: &ReceiverHost, id: &str, c: char) {
    let copy = GalleryCopy {
        locator: format!("content://media/external_primary/images/media/{}", c as u32),
        sha256: digest(id.as_bytes()),
        size: 1,
        display_name: None,
        sha1: Some(sha1(c)),
    };
    r.receiver
        .lock()
        .unwrap()
        .record_gallery_copy(id, &copy)
        .unwrap();
    rusqlite::Connection::open(t.0.join("receiver/store/receiver.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE assets SET published_at_ms=published_at_ms-3600000 WHERE id=?1",
            [id],
        )
        .unwrap();
}

fn cloud_state(r: &ReceiverHost, id: &str) -> String {
    r.receiver
        .lock()
        .unwrap()
        .status(id)
        .unwrap()
        .cloud_state
        .unwrap()
}

fn job_cloud(sender: &SenderHost, id: &str) -> Option<String> {
    sender
        .list(0)
        .unwrap()
        .into_iter()
        .find(|j| j.asset.id().unwrap() == id)
        .unwrap()
        .cloud
}

const RUN: RunOptions = RunOptions {
    dry_run: false,
    max_items: 600,
};

#[tokio::test]
async fn audit_posts_verdicts_and_mirrors_them_on_jobs() {
    let t = Temp::new();
    let r = common::start_receiver(&t.0.join("receiver"), 100000).await;
    let sender = SenderHost::open(&t.0.join("sender")).unwrap();
    let free = publish(&t, &r, &sender, "free", 'a').await;
    let quota = publish(&t, &r, &sender, "quota", 'b').await;
    let absent = publish(&t, &r, &sender, "absent", 'c').await;
    let unknown = publish(&t, &r, &sender, "unknown", 'd').await;
    let mut fake = Fake::default();
    fake.library
        .insert(sha1('a'), ("key-a".into(), Some(false)));
    fake.library.insert(sha1('b'), ("key-b".into(), Some(true)));
    fake.library.insert(sha1('d'), ("key-d".into(), None));

    // The capability is learned from an authenticated receiver probe.
    assert!(matches!(
        sender.cloud_audit(&r.pairing, &mut fake, RUN).await,
        Err(Error::Unsupported(code)) if code == "cloud_audit_unsupported"
    ));
    assert_eq!(fake.lookups, 0);
    sender
        .set_cloud_audit(&r.pairing.receiver_id, true)
        .unwrap();

    let dry = sender
        .cloud_audit(
            &r.pairing,
            &mut fake,
            RunOptions {
                dry_run: true,
                ..RUN
            },
        )
        .await
        .unwrap();
    assert_eq!(
        dry,
        RunReport {
            checked: 4,
            found: 3,
            verified: 1,
            quota: 1,
            new_quota: 1,
            not_found: 1,
            unknown: 1,
            dry_run: true,
            ..Default::default()
        }
    );
    assert_eq!(cloud_state(&r, &free), "pending");
    assert_eq!(job_cloud(&sender, &free), None);

    let report = sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    assert_eq!(
        report,
        RunReport {
            posted: 3,
            dry_run: false,
            ..dry
        }
    );
    assert_eq!(cloud_state(&r, &free), "verified");
    assert_eq!(cloud_state(&r, &quota), "verified_counts_against_quota");
    assert_eq!(cloud_state(&r, &absent), "pending");
    assert_eq!(cloud_state(&r, &unknown), "pending");
    assert_eq!(job_cloud(&sender, &free).as_deref(), Some("verified"));
    assert_eq!(
        job_cloud(&sender, &quota).as_deref(),
        Some("verified_counts_against_quota")
    );
    assert_eq!(job_cloud(&sender, &absent).as_deref(), Some("pending"));
    assert_eq!(job_cloud(&sender, &unknown), None);
    let status = sender
        .cloud_audit_status(&r.pairing.receiver_id, None)
        .unwrap();
    assert_eq!(status["configured"], false);
    assert_eq!(status["session_expired"], false);
    assert_eq!(status["last_result"]["posted"], 3);
    assert!(status["last_run_ms"].as_i64().unwrap() > 0);

    // Checked copies back off; only the unknown one is due again. Its info
    // lookup expires the session after the hash lookup: partial, not an error.
    let late = publish(&t, &r, &sender, "late", 'e').await;
    fake.expire_info = true;
    let partial = sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    assert_eq!(
        partial,
        RunReport {
            checked: 2,
            found: 1,
            not_found: 1,
            unknown: 1,
            posted: 1,
            session_expired: true,
            ..Default::default()
        }
    );
    assert_eq!(job_cloud(&sender, &late).as_deref(), Some("pending"));
    assert_eq!(
        sender
            .cloud_audit_status(&r.pairing.receiver_id, None)
            .unwrap()["session_expired"],
        true
    );

    // Nothing could be done at all: a stable error, and status remembers it.
    fake.expire_lookup = true;
    assert!(matches!(
        sender.cloud_audit(&r.pairing, &mut fake, RUN).await,
        Err(Error::Unsupported(code)) if code == "cloud_session_expired"
    ));
    let status = sender
        .cloud_audit_status(&r.pairing.receiver_id, None)
        .unwrap();
    assert_eq!(status["session_expired"], true);
    assert_eq!(status["last_result"]["checked"], 0);
    assert_eq!(cloud_state(&r, &unknown), "pending");
}

#[tokio::test]
async fn other_senders_copies_are_reported_but_not_mirrored() {
    let t = Temp::new();
    let r = common::start_receiver(&t.0.join("receiver"), 100000).await;
    let sender = SenderHost::open(&t.0.join("sender")).unwrap();
    sender
        .set_cloud_audit(&r.pairing.receiver_id, true)
        .unwrap();
    let foreign = {
        let bytes = b"foreign";
        let mut receiver = r.receiver.lock().unwrap();
        let id = receiver
            .register(Asset {
                version: PROTOCOL_VERSION,
                source_id: "other-sender".into(),
                revision: "1".into(),
                kind: AssetKind::Photo,
                metadata: BTreeMap::new(),
                resources: vec![Resource {
                    role: ResourceRole::Photo,
                    filename: "IMG_0001.JPG".into(),
                    media_type: "image/jpeg".into(),
                    size: bytes.len() as u64,
                    sha256: digest(bytes),
                }],
            })
            .unwrap()
            .asset_id;
        receiver
            .append(&id, &digest(bytes), 0, bytes, &digest(bytes))
            .unwrap();
        receiver.commit(&id).unwrap();
        id
    };
    gallery(&t, &r, &foreign, 'f');
    let mut fake = Fake::default();
    fake.library
        .insert(sha1('f'), ("key-f".into(), Some(false)));
    let report = sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    assert_eq!((report.checked, report.posted), (1, 1));
    assert_eq!(cloud_state(&r, &foreign), "verified");
    assert!(sender.list(0).unwrap().is_empty());
    // Nothing due: Google is not contacted.
    let idle = sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    assert_eq!((idle.checked, fake.lookups), (0, 1));
}

fn call(v: Value) -> Value {
    let input = CString::new(v.to_string()).unwrap();
    unsafe {
        let p = backupduck_call(input.as_ptr());
        let result = serde_json::from_str(CStr::from_ptr(p).to_str().unwrap()).unwrap();
        backupduck_free(p);
        result
    }
}

/// The FFI boundary returns stable codes and never touches Google before the
/// receiver capability and the cookies file check out.
#[test]
fn ffi_status_and_error_codes() {
    let t = Temp::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let r = rt.block_on(common::start_receiver(&t.0.join("receiver"), 100000));
    let pairing = serde_json::to_value(&r.pairing).unwrap();
    let receiver = r.pairing.receiver_id.clone();
    assert_eq!(
        call(json!({"op":"open_sender","root":t.0.join("sender")}))["ok"],
        true
    );
    let status = call(json!({"op":"cloud_audit","command":{"status":{
        "receiver_id":receiver,"cookies_path":t.0.join("missing.txt")}}}));
    assert_eq!(
        status,
        json!({"ok":true,"value":{"configured":false,"last_run_ms":null,"last_result":null,
            "session_expired":false,"last_lock_run_ms":null,"last_lock_result":null}})
    );
    let run = |cookies: PathBuf| {
        call(json!({"op":"cloud_audit","command":{"run":{
            "pairing":pairing,"cookies_path":cookies,"account_index":0}}}))
    };
    assert_eq!(
        run(t.0.join("missing.txt")),
        json!({"ok":false,"error":"cloud_audit_unsupported"})
    );
    assert_eq!(
        call(json!({"op":"check_pairing","pairing":pairing}))["value"]["cloud_audit"],
        true
    );
    assert_eq!(
        run(t.0.join("missing.txt")),
        json!({"ok":false,"error":"cloud_cookies_invalid"})
    );
    let other = t.0.join("other.txt");
    std::fs::write(&other, "example.com\tTRUE\t/\tTRUE\t0\tSID\tsecret-value\n").unwrap();
    let response = run(other.clone());
    assert_eq!(
        response,
        json!({"ok":false,"error":"cloud_cookies_invalid"})
    );
    assert!(!response.to_string().contains("secret-value"));
    // A signed-in-looking file with nothing due finishes without any lookup.
    let google = t.0.join("google.txt");
    std::fs::write(
        &google,
        ".google.com\tTRUE\t/\tTRUE\t0\tSID\tsecret-value\n",
    )
    .unwrap();
    let response = run(google.clone());
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["value"]["checked"], 0);
    assert!(!response.to_string().contains("secret-value"));
    let lock = |cookies: &PathBuf, sources: Value| {
        call(json!({"op":"cloud_audit","command":{"lock_hidden":{
            "pairing":pairing,"cookies_path":cookies,"account_index":1,
            "source_ids":sources,"dry_run":false}}}))
    };
    assert_eq!(
        lock(&t.0.join("missing.txt"), json!(["hidden"])),
        json!({"ok":false,"error":"cloud_cookies_invalid"})
    );
    // No received hidden job: done without contacting Google.
    let response = lock(&google, json!(["hidden"]));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["value"]["candidates"], 0);
    assert_eq!(response["value"]["moved"], 0);
    assert!(!response.to_string().contains("secret-value"));
    let status = call(json!({"op":"cloud_audit","command":{"status":{
        "receiver_id":receiver,"cookies_path":google}}}));
    assert_eq!(status["value"]["configured"], true);
    assert_eq!(status["value"]["last_result"]["checked"], 0);
    assert_eq!(status["value"]["last_lock_result"]["candidates"], 0);
    drop(r);
}

fn job_locked(sender: &SenderHost, id: &str) -> Option<String> {
    sender
        .list(0)
        .unwrap()
        .into_iter()
        .find(|j| j.asset.id().unwrap() == id)
        .unwrap()
        .locked_folder
}

fn lock_options(sources: &[&str], dry_run: bool) -> LockOptions {
    LockOptions {
        source_ids: sources.iter().map(|s| s.to_string()).collect(),
        dry_run,
        max_items: 200,
    }
}

#[tokio::test]
async fn lock_hidden_moves_only_verified_hidden_received_items() {
    let t = Temp::new();
    let r = common::start_receiver(&t.0.join("receiver"), 100000).await;
    let sender = SenderHost::open(&t.0.join("sender")).unwrap();
    let free = publish(&t, &r, &sender, "hidden-free", 'a').await;
    let quota = publish(&t, &r, &sender, "hidden-quota", 'b').await;
    let pending = publish(&t, &r, &sender, "hidden-pending", 'c').await;
    let visible = publish(&t, &r, &sender, "visible", 'd').await;
    let sticky = publish(&t, &r, &sender, "hidden-sticky", 'e').await;
    let gone = publish(&t, &r, &sender, "hidden-gone", 'f').await;
    let mut fake = Fake::default();
    for (c, space) in [
        ('a', false),
        ('b', true),
        ('d', false),
        ('e', false),
        ('f', false),
    ] {
        fake.library
            .insert(sha1(c), (format!("key-{c}"), Some(space)));
    }
    fake.sticky.insert(sha1('e'));
    let hidden = [
        "hidden-free",
        "hidden-quota",
        "hidden-pending",
        "hidden-sticky",
        "hidden-gone",
        "never-sent",
    ];
    assert!(matches!(
        sender.lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false)).await,
        Err(Error::Unsupported(code)) if code == "cloud_audit_unsupported"
    ));
    sender
        .set_cloud_audit(&r.pairing.receiver_id, true)
        .unwrap();
    sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    assert_eq!(cloud_state(&r, &free), "verified");
    // Gone from the library after it was verified.
    fake.library.remove(&sha1('f'));
    let lookups = fake.lookups;

    let dry = sender
        .lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, true))
        .await
        .unwrap();
    assert_eq!(
        dry,
        LockReport {
            verified_hidden: 3,
            candidates: 3,
            checked: 3,
            found: 2,
            not_in_library: 1,
            skipped_quota: 1,
            skipped_unverified: 1,
            dry_run: true,
            ..Default::default()
        }
    );
    assert!(fake.moves.is_empty());
    assert_eq!(fake.lookups, lookups + 1);
    assert_eq!(job_locked(&sender, &free), None);
    assert_eq!(job_locked(&sender, &gone), None);

    let report = sender
        .lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false))
        .await
        .unwrap();
    assert_eq!(
        report,
        LockReport {
            moved: 1,
            failed: 1,
            dry_run: false,
            ..dry.clone()
        }
    );
    assert_eq!(fake.moves, [["dedup-key-a", "dedup-key-e"]]);
    assert_eq!(job_locked(&sender, &free).as_deref(), Some("moved"));
    assert_eq!(job_locked(&sender, &sticky).as_deref(), Some("failed"));
    assert_eq!(
        job_locked(&sender, &gone).as_deref(),
        Some("not_in_library")
    );
    for id in [&quota, &pending, &visible] {
        assert_eq!(job_locked(&sender, id), None);
    }
    assert!(fake.library.contains_key(&sha1('d')));
    // Receiver state is untouched, and the audit never re-checks verified rows.
    assert_eq!(cloud_state(&r, &free), "verified");
    let status = sender
        .cloud_audit_status(&r.pairing.receiver_id, None)
        .unwrap();
    assert_eq!(status["last_lock_result"]["moved"], 1);
    assert!(status["last_lock_run_ms"].as_i64().unwrap() > 0);

    // Idempotent: the moved item is not looked up again; the sticky one is
    // retried until the attempt cap.
    for attempt in 2..=3 {
        let again = sender
            .lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false))
            .await
            .unwrap();
        assert_eq!(
            (
                again.already_moved,
                again.candidates,
                again.failed,
                again.moved
            ),
            (1, 1, 1, 0),
            "attempt {attempt}"
        );
    }
    assert_eq!(fake.moves.len(), 3);
    assert_eq!(fake.moves[2], ["dedup-key-e"]);
    let lookups = fake.lookups;
    let capped = sender
        .lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false))
        .await
        .unwrap();
    assert_eq!(
        capped,
        LockReport {
            verified_hidden: 3,
            already_moved: 1,
            attempts_exhausted: 1,
            skipped_quota: 1,
            skipped_unverified: 1,
            ..Default::default()
        }
    );
    assert_eq!(fake.lookups, lookups);
    assert_eq!(fake.moves.len(), 3);
}

#[tokio::test]
async fn lock_hidden_partial_runs_settle_on_the_next_run() {
    let t = Temp::new();
    let r = common::start_receiver(&t.0.join("receiver"), 100000).await;
    let sender = SenderHost::open(&t.0.join("sender")).unwrap();
    let mut ids = Vec::new();
    for (name, c) in [("h1", 'a'), ("h2", 'b'), ("h3", 'c'), ("h4", 'd')] {
        ids.push(publish(&t, &r, &sender, name, c).await);
    }
    let mut fake = Fake::default();
    for c in ['a', 'b', 'c', 'd'] {
        fake.library
            .insert(sha1(c), (format!("key-{c}"), Some(false)));
    }
    sender
        .set_cloud_audit(&r.pairing.receiver_id, true)
        .unwrap();
    sender
        .cloud_audit(&r.pairing, &mut fake, RUN)
        .await
        .unwrap();
    let hidden = ["h1", "h2", "h3"];

    // Nothing could be looked up: a stable error, and the run is recorded.
    fake.expire_lookup = true;
    assert!(matches!(
        sender.lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false)).await,
        Err(Error::Unsupported(code)) if code == "cloud_session_expired"
    ));
    assert_eq!(
        sender
            .cloud_audit_status(&r.pairing.receiver_id, None)
            .unwrap()["last_lock_result"]["session_expired"],
        true
    );
    fake.expire_lookup = false;

    // The move goes through but the session expires before the re-check:
    // partial counts, and nothing is recorded as moved yet.
    fake.expire_after_lookups = Some(fake.lookups + 1);
    let partial = sender
        .lock_hidden(
            &r.pairing,
            &mut fake,
            LockOptions {
                max_items: 2,
                ..lock_options(&hidden, false)
            },
        )
        .await
        .unwrap();
    assert_eq!(
        partial,
        LockReport {
            verified_hidden: 3,
            candidates: 2,
            deferred: 1,
            checked: 2,
            found: 2,
            failed: 2,
            session_expired: true,
            ..Default::default()
        }
    );
    assert_eq!(fake.moves, [["dedup-key-a", "dedup-key-b"]]);
    assert_eq!(job_locked(&sender, &ids[0]).as_deref(), Some("failed"));

    // A move that never reached Google is not recorded at all.
    fake.expire_after_lookups = None;
    fake.move_error = Some(|| AuditError::RateLimited);
    let limited = sender
        .lock_hidden(&r.pairing, &mut fake, lock_options(&["h3"], false))
        .await
        .unwrap();
    assert_eq!(
        (limited.found, limited.failed, limited.stopped.as_deref()),
        (1, 0, Some("rate_limited"))
    );
    assert_eq!(job_locked(&sender, &ids[2]), None);
    fake.move_error = None;

    // The earlier moves are no longer in the library: settled as moved
    // without a second move request; h3 moves now.
    let settled = sender
        .lock_hidden(&r.pairing, &mut fake, lock_options(&hidden, false))
        .await
        .unwrap();
    assert_eq!(
        (
            settled.candidates,
            settled.found,
            settled.moved,
            settled.failed
        ),
        (3, 1, 3, 0)
    );
    assert_eq!(fake.moves.len(), 2);
    assert_eq!(fake.moves[1], ["dedup-key-c"]);
    for id in &ids[..3] {
        assert_eq!(job_locked(&sender, id).as_deref(), Some("moved"));
    }
    assert_eq!(job_locked(&sender, &ids[3]), None);
    assert!(fake.library.contains_key(&sha1('d')));
}
