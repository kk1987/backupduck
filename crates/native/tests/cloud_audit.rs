#![cfg(feature = "cloud-audit")]
use backupduck_cloud_audit::{Error as AuditError, HashLookup, ItemInfo, Result as AuditResult};
use backupduck_core::*;
use backupduck_native::{
    backupduck_call, backupduck_free,
    cloud::{CloudLookup, RunOptions, RunReport},
    ReceiverHost, SenderHost,
};
use backupduck_store::retention::GalleryCopy;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
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
fn address() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Scripted Google library: SHA-1 -> (media key, takes up space).
#[derive(Default)]
struct Fake {
    library: HashMap<String, (String, Option<bool>)>,
    expire_lookup: bool,
    expire_info: bool,
    lookups: usize,
}
impl CloudLookup for Fake {
    async fn lookup(&mut self, sha1_hex: &[String]) -> AuditResult<Vec<Option<HashLookup>>> {
        self.lookups += 1;
        if self.expire_lookup {
            return Err(AuditError::SessionExpired);
        }
        Ok(sha1_hex
            .iter()
            .map(|h| {
                self.library.get(h).map(|(key, _)| HashLookup {
                    hash_b64: String::new(),
                    media_key: key.clone(),
                    dedup_key: None,
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
    let r = ReceiverHost::start(&t.0.join("receiver"), address(), 100000)
        .await
        .unwrap();
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
    let r = ReceiverHost::start(&t.0.join("receiver"), address(), 100000)
        .await
        .unwrap();
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
    let r = rt
        .block_on(ReceiverHost::start(
            &t.0.join("receiver"),
            address(),
            100000,
        ))
        .unwrap();
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
        json!({"ok":true,"value":{"configured":false,"last_run_ms":null,"last_result":null,"session_expired":false}})
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
    let status = call(json!({"op":"cloud_audit","command":{"status":{
        "receiver_id":receiver,"cookies_path":google}}}));
    assert_eq!(status["value"]["configured"], true);
    assert_eq!(status["value"]["last_result"]["checked"], 0);
    drop(r);
}
