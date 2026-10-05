//! A rescan or metadata-only edit must not publish a received photo again.
use backupduck_core::*;
use backupduck_native::SenderHost;
mod common;
use backupduck_sender::JobState;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
};
fn call(input: Value) -> Value {
    let input = CString::new(input.to_string()).unwrap();
    let output = unsafe { backupduck_native::backupduck_call(input.as_ptr()) };
    assert!(!output.is_null());
    let envelope: Value =
        unsafe { serde_json::from_slice(CStr::from_ptr(output).to_bytes()).unwrap() };
    unsafe { backupduck_native::backupduck_free(output) };
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["value"].clone()
}

#[test]
fn rescan_skips_sources_received_under_an_older_revision() {
    let root = std::env::temp_dir().join(format!("backupduck-rescan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let photo = root.join("photo.jpg");
    std::fs::write(&photo, b"original photo bytes").unwrap();
    let sender_root = root.join("sender");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    // Receive revision 1 over the real loopback transport.
    let (receiver, received) = runtime.block_on(async {
        let r = common::start_receiver(&root.join("receiver"), 100000).await;
        let sender = SenderHost::open(&sender_root).unwrap();
        let sha256 = digest(b"original photo bytes");
        let asset = Asset {
            version: PROTOCOL_VERSION,
            source_id: "photo-1".into(),
            revision: "1".into(),
            kind: AssetKind::Photo,
            metadata: BTreeMap::from([("favorite".into(), "false".into())]),
            resources: vec![Resource {
                role: ResourceRole::Photo,
                filename: "photo.jpg".into(),
                media_type: "image/jpeg".into(),
                size: 20,
                sha256: sha256.clone(),
            }],
        };
        let paths = BTreeMap::from([(sha256, photo.to_str().unwrap().to_string())]);
        sender
            .enqueue(&r.pairing.receiver_id, asset, paths)
            .unwrap();
        let job = sender.run_once(&r.pairing).await.unwrap().unwrap();
        assert_eq!(job.state, JobState::Received);
        (r.pairing.receiver_id.clone(), job.id)
    });
    drop(runtime);

    call(json!({"op":"open_sender","root":sender_root}));
    // Favorite toggled: new revision and asset id, identical bytes.
    let job = call(
        json!({"op":"enqueue","receiver_id":receiver,"source_id":"photo-1",
        "revision":"2","kind":"photo","metadata":{"favorite":"true"},
        "resources":[{"role":"photo","filename":"photo.jpg","media_type":"image/jpeg","path":photo}]}),
    );
    assert_eq!(job["id"], received);
    assert_eq!(
        call(json!({"op":"sender_summary","receiver_id":receiver}))["total"],
        1
    );
    assert!(
        call(json!({"op":"activity_log","receiver":false,"after":null}))
            .to_string()
            .contains("transfer_deduplicated")
    );

    let sources = json!([["photo-1", "2"]]);
    let run = call(json!({"op":"history_control","receiver_id":receiver,"action":"start"}))["run"]
        .clone();
    let status = call(
        json!({"op":"history_batch","receiver_id":receiver,"run":run,
        "sources":sources,"finished":true,"rebackup_edited":false}),
    );
    assert_eq!(
        (status["checked"].clone(), status["pending"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(
        call(json!({"op":"pending_sources","receiver_id":receiver}))["count"],
        0
    );

    // Opting in to re-upload edits schedules the source again.
    let run = call(json!({"op":"history_control","receiver_id":receiver,"action":"start"}))["run"]
        .clone();
    let status = call(
        json!({"op":"history_batch","receiver_id":receiver,"run":run,
        "sources":sources,"finished":true,"rebackup_edited":true}),
    );
    assert_eq!(status["pending"], 1);

    // A receipt in a superseded format is reported as such and rescheduled,
    // even though previous receipts would otherwise count it as done.
    call(json!({"op":"source_result","receiver_id":receiver,"source":"photo-1","complete":true}));
    assert_eq!(
        call(json!({"op":"pending_sources","receiver_id":receiver}))["count"],
        0
    );
    let received_window = json!([["photo-1", "1"]]);
    let superseded = json!(["image/jpeg"]);
    let states = call(
        json!({"op":"source_states","receiver_id":receiver,"sources":received_window,
        "include_previous_receipts":true,"superseded_media_types":superseded}),
    );
    assert_eq!(states["photo-1"], "superseded");
    let plain =
        call(json!({"op":"source_states","receiver_id":receiver,"sources":received_window}));
    assert_eq!(plain["photo-1"], "received");
    let run = call(json!({"op":"history_control","receiver_id":receiver,"action":"start"}))["run"]
        .clone();
    let status = call(
        json!({"op":"history_batch","receiver_id":receiver,"run":run,"sources":received_window,
        "finished":true,"rebackup_edited":false,"superseded_media_types":superseded}),
    );
    assert_eq!(status["pending"], 1);
    let _ = std::fs::remove_dir_all(&root);
}
