//! Exercise the real JSON boundary and source-revision index across process exit.
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    fs,
    path::Path,
    process::Command,
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
fn stage(root: &Path, phase: &str) {
    call(json!({"op":"open_sender","root":root}));
    if phase == "first" {
        let photo = root.join("fixture.jpg");
        fs::write(&photo, b"synthetic-original").unwrap();
        for i in 0..2 {
            call(
                json!({"op":"enqueue","receiver_id":"receiver-a","source_id":format!("photo-{i}"),"revision":"1","kind":"photo","resources":[{"role":"photo","filename":"fixture.jpg","media_type":"image/jpeg","path":photo}]}),
            );
        }
        call(json!({"op":"schedule_sources","receiver_id":"receiver-a","sources":["new-photo"]}));
        let run = call(json!({"op":"history_control","receiver_id":"receiver-a","action":"start"}))
            ["run"]
            .as_i64()
            .unwrap();
        for start in (0..1000).step_by(200) {
            let sources: Vec<_> = (start..start + 200)
                .map(|i| json!([format!("photo-{i}"), "1"]))
                .collect();
            call(
                json!({"op":"history_batch","receiver_id":"receiver-a","run":run,"sources":sources,"finished":false}),
            );
        }
        call(json!({"op":"history_control","receiver_id":"receiver-a","action":"pause"}));
    } else {
        let prior = call(json!({"op":"history_status","receiver_id":"receiver-a"}));
        assert_eq!(prior["state"], "paused");
        assert_eq!(prior["checked"], 1000);
        assert_eq!(prior["pending"], 998);
        call(json!({"op":"history_control","receiver_id":"receiver-a","action":"resume"}));
        // Replayed batches after process death must not duplicate pending work
        // or download existing queued revisions.
        for start in (0..10000).step_by(200) {
            let sources: Vec<_> = (start..start + 200)
                .map(|i| json!([format!("photo-{i}"), "1"]))
                .collect();
            call(
                json!({"op":"history_batch","receiver_id":"receiver-a","run":prior["run"],"sources":sources,"finished":start==9800}),
            );
        }
        let done = call(json!({"op":"history_status","receiver_id":"receiver-a"}));
        assert_eq!(done["state"], "scanned");
        assert_eq!(done["checked"], 10000);
        assert_eq!(done["pending"], 9998);
        assert_eq!(
            call(json!({"op":"pending_sources","receiver_id":"receiver-a"}))["count"],
            9999
        );
        assert_eq!(
            call(json!({"op":"sender_summary","receiver_id":"receiver-a"}))["total"],
            2
        );
        assert!(call(json!({"op":"history_status","receiver_id":"receiver-b"})).is_null());
        let next =
            call(json!({"op":"history_control","receiver_id":"receiver-a","action":"start"}));
        call(
            json!({"op":"history_batch","receiver_id":"receiver-a","run":next["run"],"sources":[["photo-0","2"],["photo-1","1"]],"finished":true}),
        );
        assert_eq!(
            call(json!({"op":"history_status","receiver_id":"receiver-a"}))["pending"],
            1
        );
    }
}
#[test]
fn ten_thousand_sources_resume_after_process_exit_without_duplicate_jobs() {
    if let Ok(root) = std::env::var("BACKUPDUCK_HISTORY_FIXTURE_ROOT") {
        stage(
            Path::new(&root),
            &std::env::var("BACKUPDUCK_HISTORY_FIXTURE_PHASE").unwrap(),
        );
        return;
    }
    let root = std::env::temp_dir().join(format!("backupduck-history-ffi-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for phase in ["first", "second"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ten_thousand_sources_resume_after_process_exit_without_duplicate_jobs",
                "--nocapture",
            ])
            .env("BACKUPDUCK_HISTORY_FIXTURE_ROOT", &root)
            .env("BACKUPDUCK_HISTORY_FIXTURE_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}
fn park(root: &Path, phase: &str) {
    call(json!({"op":"open_sender","root":root}));
    let receiver = "receiver-a";
    let sources = json!([["broken", "1"], ["fine", "1"]]);
    if phase == "first" {
        let run = call(json!({"op":"history_control","receiver_id":receiver,"action":"start"}))
            ["run"]
            .clone();
        call(
            json!({"op":"history_batch","receiver_id":receiver,"run":run,"sources":sources,"finished":true}),
        );
        for _ in 0..5 {
            call(
                json!({"op":"source_result","receiver_id":receiver,"source":"broken","complete":false,"error":"unsupported"}),
            );
        }
        // Older clients omit the error; it counts as item-specific but does not park yet.
        call(json!({"op":"source_result","receiver_id":receiver,"source":"fine","complete":false}));
        return;
    }
    let pending = call(json!({"op":"pending_sources","receiver_id":receiver}));
    assert_eq!(
        (pending["count"].clone(), pending["needs_attention"].clone()),
        (json!(1), json!(1))
    );
    // A new history scan reschedules members but must not revive parked work.
    let run = call(json!({"op":"history_control","receiver_id":receiver,"action":"start"}))["run"]
        .clone();
    call(
        json!({"op":"history_batch","receiver_id":receiver,"run":run,"sources":sources,"finished":true}),
    );
    let pending = call(json!({"op":"pending_sources","receiver_id":receiver}));
    assert_eq!(
        (pending["count"].clone(), pending["needs_attention"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(
        call(json!({"op":"history_status","receiver_id":receiver}))["pending"],
        1
    );
    let parked = call(json!({"op":"needs_attention","receiver_id":receiver}));
    assert_eq!(parked["total"], 1);
    assert_eq!(parked["items"][0]["source"], "broken");
    assert_eq!(parked["items"][0]["error_code"], "unsupported");
    assert_eq!(parked["items"][0]["attempts"], 5);
    let history = call(json!({"op":"browse_sources","receiver_id":receiver,"history":true}));
    let row = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["source"] == "broken")
        .unwrap();
    assert_eq!(row["state"], "needs_attention");
    assert_eq!(row["error_code"], "unsupported");
    let delayed = call(json!({"op":"browse_sources","receiver_id":receiver,"history":false}));
    assert_eq!(delayed["total"], 1);
    assert_eq!(delayed["items"][0]["source"], "fine");
    assert_eq!(
        call(json!({"op":"retry_source","receiver_id":receiver,"source":"broken"}))["count"],
        1
    );
    let pending = call(json!({"op":"pending_sources","receiver_id":receiver}));
    assert_eq!(
        (pending["count"].clone(), pending["needs_attention"].clone()),
        (json!(2), json!(0))
    );
    assert_eq!(
        call(json!({"op":"skip_source","receiver_id":receiver,"source":"broken"}))["count"],
        1
    );
    assert_eq!(
        call(json!({"op":"retry_all_needs_attention","receiver_id":receiver}))["count"],
        0
    );
}
#[test]
fn parked_sources_survive_restart_and_rescan() {
    if let Ok(root) = std::env::var("BACKUPDUCK_PARKED_FIXTURE_ROOT") {
        park(
            Path::new(&root),
            &std::env::var("BACKUPDUCK_PARKED_FIXTURE_PHASE").unwrap(),
        );
        return;
    }
    let root = std::env::temp_dir().join(format!("backupduck-parked-ffi-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for phase in ["first", "second"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "parked_sources_survive_restart_and_rescan",
                "--nocapture",
            ])
            .env("BACKUPDUCK_PARKED_FIXTURE_ROOT", &root)
            .env("BACKUPDUCK_PARKED_FIXTURE_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}
