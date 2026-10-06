//! Cloud release gating through the native command boundary.
use backupduck_core::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "backupduck-cloud-release-{}-{}",
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
fn call(value: Value) -> Value {
    serde_json::from_str(&backupduck_native::call(&value.to_string())).unwrap()
}
fn ok(value: Value) -> Value {
    let output = call(value);
    assert_eq!(output["ok"], true, "{output}");
    output["value"].clone()
}
fn received(root: &Path, name: &str) -> String {
    let mut receiver = backupduck_store::Receiver::open(root.join("store"), 1 << 20).unwrap();
    let bytes = name.as_bytes();
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
    let id = receiver.register(asset).unwrap().asset_id;
    receiver
        .append(&id, &digest(bytes), 0, bytes, &digest(bytes))
        .unwrap();
    receiver.commit(&id).unwrap();
    id
}
fn copy(n: u8) -> Value {
    json!({
        "locator": format!("content://media/external_primary/images/media/{n}"),
        "sha256": digest(&[n]),
        "size": 1,
        "display_name": format!("IMG_{n}.JPG"),
        "sha1": n.to_string().repeat(40),
    })
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn cloud_release_supersedes_relay_and_releases_only_verified_copies() {
    let t = Temp::new();
    let root = t.0.join("receiver");
    // Settings saved before cloud release existed still load.
    let old = json!({"cache_budget_bytes":1u64<<30,"receiver_budget_bytes":1u64<<30,"min_free_bytes":0,
        "auto_reclaim":true,"receiver_relay":true,"log_days":14,"log_limit":5000});
    let settings =
        ok(json!({"op":"receiver_settings","root":root,"settings":old}))["settings"].clone();
    assert_eq!(settings["cloud_release"], false);
    assert_eq!(settings["cloud_release_grace_ms"], 3_600_000);
    let save = |enabled: bool| {
        let mut s = settings.clone();
        s["cloud_release"] = json!(enabled);
        s["cloud_release_grace_ms"] = json!(0);
        ok(json!({"op":"receiver_settings","root":root,"settings":s}));
    };
    let mut bad = settings.clone();
    bad["cloud_release_grace_ms"] = json!(7u64 * 86_400_000 + 1);
    assert_eq!(
        call(json!({"op":"receiver_settings","root":root,"settings":bad}))["ok"],
        false
    );

    let first = received(&root, "first");
    let second = received(&root, "second");
    let candidates = || ok(json!({"op":"cloud_release_candidates","root":root}));
    // Off: no candidates, and no release even for a verified copy.
    assert_eq!(candidates(), json!([]));
    save(true);
    let published = ok(json!({"op":"gallery_publication","root":root,"id":first,"copy":copy(1)}));
    assert_eq!(
        published["bytes"], 0,
        "relay must not release while cloud release is on"
    );
    assert_eq!(
        ok(json!({"op":"gallery_candidates","root":root})),
        json!([])
    );
    assert_eq!(
        ok(json!({"op":"gallery_candidates","root":root,"manual":true})),
        json!([])
    );
    assert_eq!(
        call(json!({"op":"release_gallery","root":root,"id":first,"copy":copy(1),"manual":true}))
            ["error"],
        "conflict"
    );
    assert_eq!(candidates(), json!([]), "pending copies are not eligible");

    let mut store = backupduck_store::Receiver::open(root.join("store"), 1 << 20).unwrap();
    let summary = store
        .observe_cloud(
            &[CloudObservation {
                asset_id: first.clone(),
                sha1: "1".repeat(40),
                result: CloudResult::Free,
                media_key: None,
                device_model: None,
            }],
            now_ms() - 1,
        )
        .unwrap();
    assert_eq!(summary.verified, 1);
    drop(store);
    let listed = candidates();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], first.as_str());
    assert_eq!(listed[0]["copy"], copy(1));

    save(false);
    assert_eq!(candidates(), json!([]));
    let release = |missing: bool| {
        call(
            json!({"op":"release_cloud_verified","root":root,"id":first,"copy":copy(1),"missing":missing}),
        )
    };
    assert_eq!(release(false)["error"], "conflict");
    save(true);
    let mut wrong = copy(1);
    wrong["sha1"] = json!("2".repeat(40));
    assert_eq!(
        call(json!({"op":"release_cloud_verified","root":root,"id":first,"copy":wrong}))["error"],
        "integrity"
    );
    assert_eq!(release(false)["value"]["bytes"], "first".len());
    assert_eq!(release(false)["value"]["bytes"], 0);
    let mark = |reason: &str| {
        call(json!({"op":"mark_gallery_released","root":root,"id":first,"reason":reason}))
    };
    assert_eq!(mark("other")["ok"], false);
    assert_eq!(mark("cloud")["value"]["updated"], true);
    assert_eq!(mark("cloud")["value"]["updated"], false);
    assert_eq!(candidates(), json!([]));
    let history = ok(json!({"op":"receiver_history","root":root,"state":"all","kind":"all"}));
    let item = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == first.as_str())
        .unwrap();
    assert_eq!(
        (
            &item["originals_released"],
            &item["release_reason"],
            &item["gallery_released"],
            &item["cloud_state"]
        ),
        (
            &json!(true),
            &json!("cloud"),
            &json!(true),
            &json!("verified")
        )
    );
    let logs = ok(json!({"op":"receiver_logs","root":root}));
    let codes: Vec<&str> = logs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["code"].as_str())
        .collect();
    assert!(codes.contains(&"cloud_originals_released"), "{codes:?}");
    assert!(codes.contains(&"cloud_gallery_released"), "{codes:?}");

    // Off again: RC.3 relay behaviour is unchanged.
    save(false);
    let relayed = ok(json!({"op":"gallery_publication","root":root,"id":second,"copy":copy(2)}));
    assert_eq!(relayed["bytes"], "second".len());
}

#[test]
fn early_release_frees_originals_at_publication_and_keeps_the_copy_for_the_cloud() {
    let t = Temp::new();
    let root = t.0.join("receiver");
    let base = json!({"cache_budget_bytes":1u64<<30,"receiver_budget_bytes":1u64<<30,"min_free_bytes":0,
        "auto_reclaim":true,"receiver_relay":false,"log_days":14,"log_limit":5000});
    let settings =
        ok(json!({"op":"receiver_settings","root":root,"settings":base}))["settings"].clone();
    assert_eq!(settings["release_originals_on_publication"], false);
    let save = |cloud: bool, early: bool| {
        let mut s = settings.clone();
        s["cloud_release"] = json!(cloud);
        s["cloud_release_grace_ms"] = json!(0);
        s["release_originals_on_publication"] = json!(early);
        ok(json!({"op":"receiver_settings","root":root,"settings":s}));
    };
    let blob = |name: &str| root.join("store/blobs").join(digest(name.as_bytes()));
    let older = received(&root, "older");
    let newer = received(&root, "newer");
    let gated = received(&root, "gated");

    // Early release alone does nothing without cloud release.
    save(false, true);
    let published = ok(json!({"op":"gallery_publication","root":root,"id":gated,"copy":copy(3)}));
    assert_eq!(published["bytes"], 0);
    assert!(blob("gated").exists());

    save(true, false);
    ok(json!({"op":"gallery_publication","root":root,"id":older,"copy":copy(1)}));
    assert!(blob("older").exists());

    save(true, true);
    let published = ok(json!({"op":"gallery_publication","root":root,"id":newer,"copy":copy(2)}));
    assert_eq!(published["bytes"], "newer".len());
    assert!(!blob("newer").exists());
    // The next sweep catches up assets published before the switch.
    assert_eq!(
        ok(json!({"op":"cloud_release_candidates","root":root})),
        json!([])
    );
    assert!(!blob("older").exists());
    assert!(!blob("gated").exists());

    // The gallery copy still waits for the cloud and is released as before.
    let mut store = backupduck_store::Receiver::open(root.join("store"), 1 << 20).unwrap();
    store
        .observe_cloud(
            &[CloudObservation {
                asset_id: newer.clone(),
                sha1: "2".repeat(40),
                result: CloudResult::Free,
                media_key: None,
                device_model: None,
            }],
            now_ms() - 1,
        )
        .unwrap();
    drop(store);
    let candidates = ok(json!({"op":"cloud_release_candidates","root":root}));
    assert_eq!(candidates.as_array().unwrap().len(), 1, "{candidates}");
    assert_eq!(candidates[0]["id"], newer);
}

fn received_as(root: &Path, source: &str, bytes: &[u8], name: &str, media_type: &str) -> String {
    let mut receiver = backupduck_store::Receiver::open(root.join("store"), 1 << 20).unwrap();
    let asset = Asset {
        version: PROTOCOL_VERSION,
        source_id: source.into(),
        revision: "1".into(),
        kind: AssetKind::Photo,
        metadata: BTreeMap::new(),
        resources: vec![Resource {
            role: ResourceRole::Photo,
            filename: name.into(),
            media_type: media_type.into(),
            size: bytes.len() as u64,
            sha256: digest(bytes),
        }],
    };
    let id = receiver.register(asset).unwrap().asset_id;
    receiver
        .append(&id, &digest(bytes), 0, bytes, &digest(bytes))
        .unwrap();
    receiver.commit(&id).unwrap();
    id
}

#[test]
fn avif_copy_is_released_once_its_heic_replacement_is_verified() {
    let t = Temp::new();
    let root = t.0.join("receiver");
    let base = json!({"cache_budget_bytes":1u64<<30,"receiver_budget_bytes":1u64<<30,"min_free_bytes":0,
        "auto_reclaim":true,"receiver_relay":false,"log_days":14,"log_limit":5000});
    let mut settings =
        ok(json!({"op":"receiver_settings","root":root,"settings":base}))["settings"].clone();
    settings["cloud_release"] = json!(true);
    settings["cloud_release_grace_ms"] = json!(0);
    ok(json!({"op":"receiver_settings","root":root,"settings":settings}));
    let avif = received_as(&root, "photo-7", b"avif bytes", "P7.avif", "image/avif");
    let heic = received_as(&root, "photo-7", b"heic bytes", "P7.heic", "image/heic");
    let other = received_as(&root, "photo-8", b"other avif", "P8.avif", "image/avif");
    for (id, n) in [(&avif, 1u8), (&heic, 2), (&other, 3)] {
        ok(json!({"op":"gallery_publication","root":root,"id":id,"copy":copy(n)}));
    }
    let candidates = || ok(json!({"op":"superseded_candidates","root":root}));
    // Not superseded until the HEIC copy is verified in the cloud.
    assert_eq!(candidates(), json!([]));
    let mut store = backupduck_store::Receiver::open(root.join("store"), 1 << 20).unwrap();
    store
        .observe_cloud(
            &[CloudObservation {
                asset_id: heic.clone(),
                sha1: "2".repeat(40),
                result: CloudResult::Free,
                media_key: None,
                device_model: None,
            }],
            now_ms() - 1,
        )
        .unwrap();
    drop(store);
    let found = candidates();
    assert_eq!(found.as_array().unwrap().len(), 1, "{found}");
    assert_eq!(found[0]["id"], avif);
    // A copy that does not match the stored evidence is refused.
    assert_eq!(
        call(json!({"op":"release_superseded","root":root,"id":avif,"copy":copy(9)}))["error"],
        "integrity"
    );
    let released = ok(json!({"op":"release_superseded","root":root,"id":avif,"copy":copy(1)}));
    assert_eq!(released["bytes"], b"avif bytes".len());
    assert_eq!(
        ok(json!({"op":"mark_superseded","root":root,"id":avif}))["updated"],
        true
    );
    assert_eq!(candidates(), json!([]));
    // The other source has no verified replacement and stays.
    assert_eq!(
        call(json!({"op":"release_superseded","root":root,"id":other,"copy":copy(3)}))["error"],
        "conflict"
    );
    // Off: nothing is offered.
    settings["cloud_release"] = json!(false);
    ok(json!({"op":"receiver_settings","root":root,"settings":settings}));
    assert_eq!(candidates(), json!([]));
}
