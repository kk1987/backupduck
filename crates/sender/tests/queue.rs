use backupduck_core::*;
use backupduck_sender::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static SEQ: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "backupduck-queue-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn asset() -> Asset {
    Asset {
        version: PROTOCOL_VERSION,
        source_id: "native:42".into(),
        revision: "1".into(),
        kind: AssetKind::Photo,
        metadata: BTreeMap::new(),
        resources: vec![Resource {
            role: ResourceRole::Photo,
            filename: "photo.jpg".into(),
            media_type: "image/jpeg".into(),
            size: 10,
            sha256: digest(b"0123456789"),
        }],
    }
}
fn add(s: &mut Sender) -> Job {
    let a = asset();
    let sources = BTreeMap::from([(
        a.resources[0].sha256.clone(),
        "opaque-native-resource-reference".into(),
    )]);
    s.enqueue("receiver-1", a, sources).unwrap()
}
fn status(j: &Job, received: bool) -> AssetStatus {
    AssetStatus {
        asset_id: j.asset.id().unwrap(),
        receipt: if received {
            ReceiptState::Received
        } else {
            ReceiptState::Receiving
        },
        processing: ProcessingState::NotRequested,
        processing_error: None,
        cloud_state: None,
        resources: vec![ResourceStatus {
            sha256: j.asset.resources[0].sha256.clone(),
            offset: if received { 10 } else { 5 },
            complete: received,
        }],
    }
}

#[test]
fn explicit_rebackup_creates_a_new_job_without_changing_source_revision() {
    let t = Temp::new();
    let mut sender = Sender::open(&t.0).unwrap();
    let first = add(&mut sender);
    let running = sender.claim("receiver-1", 0).unwrap().unwrap();
    sender
        .acknowledge(&running.attempt(), &status(&running, true))
        .unwrap();
    assert_eq!(sender.job(first.id).unwrap().state, JobState::Received);

    let mut repeat = asset();
    repeat
        .metadata
        .insert("backupduck_rebackup_id".into(), "manual-request-1".into());
    let sources = BTreeMap::from([(
        repeat.resources[0].sha256.clone(),
        "opaque-native-resource-reference".into(),
    )]);
    let second = sender.enqueue("receiver-1", repeat, sources).unwrap();
    assert_ne!(first.id, second.id);
    assert_ne!(first.asset.id().unwrap(), second.asset.id().unwrap());
    assert_eq!(second.state, JobState::Queued);
    assert_eq!(second.asset.source_id, first.asset.source_id);
    assert_eq!(second.asset.revision, first.asset.revision);
    assert_eq!(sender.job(first.id).unwrap().state, JobState::Received);
    assert_eq!(
        sender
            .source_states("receiver-1", &[("native:42".into(), "1".into())])
            .unwrap()["native:42"],
        "queued"
    );
}

#[test]
fn identical_content_reenqueue_returns_received_job() {
    let t = Temp::new();
    let mut sender = Sender::open(&t.0).unwrap();
    let first = add(&mut sender);
    let running = sender.claim("receiver-1", 0).unwrap().unwrap();
    sender
        .acknowledge(&running.attempt(), &status(&running, true))
        .unwrap();
    let enqueue = |s: &mut Sender, a: Asset| {
        let sources = BTreeMap::from([(a.resources[0].sha256.clone(), "new-export".into())]);
        s.enqueue("receiver-1", a, sources).unwrap()
    };

    // Metadata-only edit: new revision and asset id, same bytes.
    let mut edited = asset();
    edited.revision = "2".into();
    edited.metadata.insert("favorite".into(), "true".into());
    assert_ne!(edited.id().unwrap(), first.asset.id().unwrap());
    let same = enqueue(&mut sender, edited.clone());
    assert_eq!(same.id, first.id);
    assert_eq!(same.state, JobState::Received);
    assert_eq!(same.sources, first.sources);
    assert_eq!(sender.summary("receiver-1").unwrap()["total"], 1);

    // Another receiver has no receipt for these bytes.
    let sources = BTreeMap::from([(edited.resources[0].sha256.clone(), "x".into())]);
    let other = sender
        .enqueue("receiver-2", edited.clone(), sources)
        .unwrap();
    assert_ne!(other.id, first.id);

    // Changed bytes are new content.
    let mut changed = edited.clone();
    changed.resources[0].sha256 = digest(b"edited-bytes");
    let new = enqueue(&mut sender, changed);
    assert_ne!(new.id, first.id);
    assert_eq!(new.state, JobState::Queued);

    // An explicit repeat backup always creates a distinct asset.
    let mut repeat = edited;
    repeat
        .metadata
        .insert("backupduck_rebackup_id".into(), "manual-request-1".into());
    let repeated = enqueue(&mut sender, repeat);
    assert_ne!(repeated.id, first.id);
    assert_ne!(repeated.id, new.id);
    assert_eq!(sender.summary("receiver-1").unwrap()["total"], 3);
}

#[test]
fn gallery_failure_stays_visible_and_retry_never_requeues_original_bytes() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    add(&mut s);
    let j = s.claim("receiver-1", 0).unwrap().unwrap();
    let mut receipt = status(&j, true);
    receipt.processing = ProcessingState::Pending;
    s.acknowledge(&j.attempt(), &receipt).unwrap();
    assert_eq!(
        s.job(j.id).unwrap().processing,
        Some(ProcessingState::Pending)
    );
    assert_eq!(s.processing_due("receiver-1", 1, 10).unwrap().len(), 1);
    assert!(s.claim("receiver-1", 1).unwrap().is_none());

    receipt.processing = ProcessingState::Failed;
    receipt.processing_error = Some("unsupported".into());
    s.observe_processing(j.id, &receipt, 2).unwrap();
    assert_eq!(
        s.job(j.id).unwrap().processing_error.as_deref(),
        Some("unsupported")
    );
    assert_eq!(s.summary("receiver-1").unwrap()["publication_failed"], 1);
    assert!(s.processing_due("receiver-1", 20, 10).unwrap().is_empty());

    receipt.processing = ProcessingState::Complete;
    receipt.processing_error = None;
    s.observe_processing(j.id, &receipt, 63).unwrap();
    drop(s);
    let mut s = Sender::open(&t.0).unwrap();
    assert_eq!(s.job(j.id).unwrap().state, JobState::Received);
    assert_eq!(
        s.job(j.id).unwrap().processing,
        Some(ProcessingState::Complete)
    );
    assert_eq!(s.summary("receiver-1").unwrap()["published"], 1);
    assert!(s.processing_due("receiver-1", 1000, 10).unwrap().is_empty());
    assert!(s.claim("receiver-1", 1000).unwrap().is_none());
}

#[test]
fn capture_order_survives_restart_and_keeps_running_and_delayed_jobs() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let mut ids = Vec::new();
    for (name, date) in [
        ("old", Some(100)),
        ("new", Some(900)),
        ("middle", Some(500)),
        ("unknown", None),
    ] {
        let mut a = asset();
        a.source_id = name.into();
        if let Some(date) = date {
            a.metadata.insert("created_at_ms".into(), date.to_string());
        }
        let sources = BTreeMap::from([(a.resources[0].sha256.clone(), "resource".into())]);
        ids.push(s.enqueue("receiver-1", a, sources).unwrap().id);
    }
    drop(s);
    let mut s = Sender::open(&t.0).unwrap();
    let newest = s.claim("receiver-1", 10).unwrap().unwrap();
    assert_eq!(newest.id, ids[1]);
    s.fail(&newest.attempt(), Failure::Network, 10).unwrap();
    let retry_at = s.job(newest.id).unwrap().next_attempt_at;
    let middle = s.claim("receiver-1", 10).unwrap().unwrap();
    assert_eq!(middle.id, ids[2]);
    // An eligible retry follows capture order; an existing upload is not reset.
    assert_eq!(s.claim("receiver-1", retry_at).unwrap().unwrap().id, ids[1]);
    assert_eq!(s.job(middle.id).unwrap().state, JobState::Running);
    assert_eq!(s.claim("receiver-1", retry_at).unwrap().unwrap().id, ids[0]);
    assert_eq!(s.claim("receiver-1", retry_at).unwrap().unwrap().id, ids[3]);
}

#[test]
fn concurrency_setting_persists_and_does_not_cancel_active_tasks() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    assert_eq!(s.concurrent_uploads().unwrap(), 4);
    assert!(s.set_concurrent_uploads(0).is_err());
    assert!(s.set_concurrent_uploads(5).is_err());
    s.set_concurrent_uploads(2).unwrap();
    s.set_bundle_upload("receiver-1", true).unwrap();
    for n in 0..4 {
        let mut a = asset();
        a.source_id = format!("concurrent-{n}");
        let sources = BTreeMap::from([(a.resources[0].sha256.clone(), "resource".into())]);
        s.enqueue("receiver-1", a, sources).unwrap();
    }
    let first = s.claim_native("receiver-1", 1).unwrap().unwrap();
    let second = s.claim_native("receiver-1", 1).unwrap().unwrap();
    assert!(s.claim_native("receiver-1", 1).unwrap().is_none());
    s.set_concurrent_uploads(1).unwrap();
    assert_eq!(s.job(first.id).unwrap().state, JobState::Running);
    assert_eq!(s.job(second.id).unwrap().state, JobState::Running);
    assert!(s.claim_native("receiver-1", 1).unwrap().is_none());
    s.acknowledge(&first.attempt(), &status(&first, true))
        .unwrap();
    assert!(s.claim_native("receiver-1", 1).unwrap().is_none());
    s.acknowledge(&second.attempt(), &status(&second, true))
        .unwrap();
    assert!(s.claim_native("receiver-1", 1).unwrap().is_some());
    drop(s);
    let s = Sender::open(&t.0).unwrap();
    assert_eq!(s.concurrent_uploads().unwrap(), 1);
}
#[test]
fn restart_resumes_and_keeps_pause_progress_and_dedupe() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let id = add(&mut s).id;
    assert_eq!(id, add(&mut s).id);
    let j = s.claim("receiver-1", 10).unwrap().unwrap();
    s.acknowledge(&j.attempt(), &status(&j, false)).unwrap();
    s.set_paused(true).unwrap();
    drop(s);
    let mut s = Sender::open(&t.0).unwrap();
    assert!(s.claim("receiver-1", 11).unwrap().is_none());
    assert_eq!(s.job(id).unwrap().confirmed_bytes, 5);
    s.set_paused(false).unwrap();
    let next = s.claim("receiver-1", 12).unwrap().unwrap();
    assert!(next.generation > j.generation);
    assert!(s.acknowledge(&j.attempt(), &status(&j, true)).is_err());
    s.acknowledge(&next.attempt(), &status(&next, true))
        .unwrap();
    s.retry(id).unwrap();
    assert_eq!(s.job(id).unwrap().state, JobState::Received);
    assert!(s.claim("receiver-1", 999).unwrap().is_none());
}
#[test]
fn transient_failure_is_automatic_permanent_failure_requires_action() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let id = add(&mut s).id;
    for f in [
        Failure::Network,
        Failure::Capacity,
        Failure::LowSpace,
        Failure::Busy,
    ] {
        let j = s.claim("receiver-1", 10000).unwrap().unwrap();
        s.fail(&j.attempt(), f, 10000).unwrap();
        let waiting = s.job(id).unwrap();
        assert_eq!(waiting.state, JobState::Waiting);
        assert!(s
            .claim("receiver-1", waiting.next_attempt_at - 1)
            .unwrap()
            .is_none());
        let j = s
            .claim("receiver-1", waiting.next_attempt_at)
            .unwrap()
            .unwrap();
        s.interrupted(&j.attempt()).unwrap();
    }
    let j = s.claim("receiver-1", 20000).unwrap().unwrap();
    s.fail(&j.attempt(), Failure::Authentication, 20000)
        .unwrap();
    assert!(s.claim("receiver-1", i64::MAX).unwrap().is_none());
    s.retry(id).unwrap();
    assert!(s.claim("receiver-1", 20001).unwrap().is_some());
}
#[test]
fn oversized_asset_capacity_is_parked_not_retried() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let id = add(&mut s).id;
    // Shared budget pressure clears as other content is released.
    let j = s.claim("receiver-1", 100).unwrap().unwrap();
    s.fail(&j.attempt(), Failure::from_error(&Error::Capacity), 100)
        .unwrap();
    assert_eq!(s.job(id).unwrap().state, JobState::Waiting);
    let due = s.job(id).unwrap().next_attempt_at;
    let j = s.claim("receiver-1", due).unwrap().unwrap();
    // An asset larger than the whole budget never fits on its own.
    s.fail(
        &j.attempt(),
        Failure::from_error(&Error::ExceedsCapacity),
        due,
    )
    .unwrap();
    let parked = s.job(id).unwrap();
    assert_eq!(parked.state, JobState::Failed);
    assert_eq!(
        parked.error_code.as_deref(),
        Some("receiver_budget_single_item")
    );
    assert!(s.claim("receiver-1", i64::MAX).unwrap().is_none());
    // Raising the budget is followed by an explicit retry.
    s.retry(id).unwrap();
    assert!(s.claim("receiver-1", due).unwrap().is_some());
}
#[test]
fn native_task_survives_restart_and_lost_completion_queries_receiver() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    add(&mut s);
    let j = s.claim("receiver-1", 0).unwrap().unwrap();
    s.bind_native_task(&j.attempt(), "session:42").unwrap();
    drop(s);
    let mut s = Sender::open(&t.0).unwrap();
    assert!(s.claim("receiver-1", 0).unwrap().is_none());
    let cancel = s
        .reconcile_native(&BTreeSet::from([
            "session:42".into(),
            "session:stale".into(),
        ]))
        .unwrap();
    assert_eq!(cancel, vec!["session:stale"]);
    assert!(s.claim("receiver-1", 0).unwrap().is_none());
    s.reconcile_native(&BTreeSet::new()).unwrap();
    let next = s.claim("receiver-1", 0).unwrap().unwrap();
    assert!(s.acknowledge(&j.attempt(), &status(&j, true)).is_err());
    s.acknowledge(&next.attempt(), &status(&next, true))
        .unwrap();
}
#[test]
fn pause_invalidates_native_callbacks_and_multiple_destinations_are_independent() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let first = add(&mut s);
    let second = s
        .enqueue("receiver-2", first.asset.clone(), first.sources.clone())
        .unwrap();
    assert_ne!(first.id, second.id);
    let j = s.claim("receiver-1", 0).unwrap().unwrap();
    s.bind_native_task(&j.attempt(), "session:task").unwrap();
    assert_eq!(s.pause_job(j.id).unwrap().as_deref(), Some("session:task"));
    assert!(s.acknowledge(&j.attempt(), &status(&j, true)).is_err());
    assert!(s.claim("receiver-1", 999).unwrap().is_none());
    assert!(s.claim("receiver-2", 0).unwrap().is_some());
}
#[test]
fn invalid_ack_cannot_complete_and_root_has_single_owner() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    assert!(Sender::open(&t.0).is_err());
    add(&mut s);
    let j = s.claim("receiver-1", 0).unwrap().unwrap();
    let mut st = status(&j, true);
    st.asset_id = digest(b"other");
    assert!(s.acknowledge(&j.attempt(), &st).is_err());
    assert_eq!(s.job(j.id).unwrap().state, JobState::Running);
}

#[test]
fn reexport_repairs_missing_sources_without_duplicating_the_asset() {
    let t = Temp::new();
    let mut sender = Sender::open(&t.0).unwrap();
    let first = add(&mut sender);
    let running = sender.claim("receiver-1", 0).unwrap().unwrap();
    sender
        .fail(&running.attempt(), Failure::SourceUnavailable, 0)
        .unwrap();
    let replacement = BTreeMap::from([(
        first.asset.resources[0].sha256.clone(),
        "new-export-reference".into(),
    )]);
    let repaired = sender
        .enqueue("receiver-1", first.asset, replacement.clone())
        .unwrap();
    assert_eq!(repaired.id, first.id);
    assert_eq!(repaired.sources, replacement);
    assert_eq!(repaired.state, JobState::Queued);
    assert_eq!(sender.list(0, 500).unwrap().len(), 1);
    assert!(sender
        .acknowledge(&running.attempt(), &status(&running, true))
        .is_err());
}

#[test]
fn native_ack_is_atomic_and_only_its_bound_attempt_can_advance() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    add(&mut s);
    let j = s.claim_native("receiver-1", 0).unwrap().unwrap();
    assert!(s.claim_native("receiver-1", 0).unwrap().is_none());
    s.bind_native_task(&j.attempt(), "42").unwrap();
    assert!(s.bind_native_task(&j.attempt(), "other").is_err());
    assert!(s
        .complete_native(&j.attempt(), "other", &status(&j, false))
        .is_err());
    s.complete_native(&j.attempt(), "42", &status(&j, false))
        .unwrap();
    drop(s);
    let mut s = Sender::open(&t.0).unwrap();
    assert_eq!(s.checkpoint(j.id).unwrap().unwrap().resources[0].offset, 5);
    assert_eq!(s.job(j.id).unwrap().attempts, 0);
    let next = s.claim_native("receiver-1", 0).unwrap().unwrap();
    assert!(s
        .complete_native(&j.attempt(), "42", &status(&j, true))
        .is_err());
    s.set_paused(true).unwrap();
    assert!(s.bind_native_task(&next.attempt(), "43").is_err());
    s.interrupted(&next.attempt()).unwrap();
    assert!(s.checkpoint(j.id).unwrap().is_none());
    assert!(s.claim_native("receiver-1", 999).unwrap().is_none());
}

#[test]
fn visible_source_status_is_scoped_to_receiver_and_revision() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let before = s.revision().unwrap();
    let first = add(&mut s);
    assert!(s.revision().unwrap() > before);
    let keys = vec![(first.asset.source_id.clone(), first.asset.revision.clone())];
    assert_eq!(
        s.source_states("receiver-1", &keys).unwrap()[&first.asset.source_id],
        "queued"
    );
    assert!(s.source_states("receiver-2", &keys).unwrap().is_empty());
    assert!(s
        .source_states(
            "receiver-1",
            &[(first.asset.source_id.clone(), "edited".into())]
        )
        .unwrap()
        .is_empty());
    let job = s.claim("receiver-1", 0).unwrap().unwrap();
    s.acknowledge(&job.attempt(), &status(&job, true)).unwrap();
    assert_eq!(
        s.source_states("receiver-1", &keys).unwrap()[&first.asset.source_id],
        "received"
    );
    let edited = vec![(first.asset.source_id.clone(), "edited".into())];
    assert!(s.source_states("receiver-1", &edited).unwrap().is_empty());
    assert_eq!(
        s.previous_receipts("receiver-1", &edited).unwrap()[&first.asset.source_id],
        "received_previous"
    );
    assert!(s
        .previous_receipts("receiver-2", &edited)
        .unwrap()
        .is_empty());
    let summary = s.summary("receiver-1").unwrap();
    assert_eq!(summary["received"], 1);
    assert_eq!(summary["confirmed_bytes"], 10);
    assert_eq!(s.summary("receiver-2").unwrap()["total"], 0);
    assert!(s
        .source_states("receiver-1", &vec![keys[0].clone(); 401])
        .is_err());
}

#[test]
fn filtering_precedes_task_pagination() {
    let t = Temp::new();
    let mut sender = Sender::open(&t.0).unwrap();
    for index in 0..205 {
        let mut a = asset();
        a.source_id = format!("source-{index}");
        sender
            .enqueue(
                "receiver-1",
                a,
                BTreeMap::from([(digest(b"0123456789"), "resource".into())]),
            )
            .unwrap();
    }
    let mut a = asset();
    a.source_id = "other-receiver".into();
    let other = sender
        .enqueue(
            "receiver-2",
            a,
            BTreeMap::from([(digest(b"0123456789"), "resource".into())]),
        )
        .unwrap();
    assert_eq!(
        sender
            .list_filtered(0, 200, Some("receiver-2"), None)
            .unwrap()[0]
            .id,
        other.id
    );
    assert_eq!(
        sender
            .list_filtered(0, 200, Some("receiver-1"), None)
            .unwrap()
            .len(),
        200
    );
    assert_eq!(
        sender
            .list_filtered(200, 200, Some("receiver-1"), None)
            .unwrap()
            .len(),
        5
    );
    let mut ascending = Vec::new();
    let mut descending = Vec::new();
    for (reverse, ids) in [(false, &mut ascending), (true, &mut descending)] {
        let mut cursor = 0;
        loop {
            let page = sender
                .list_filtered_ordered(cursor, 100, Some("receiver-1"), Some("queued"), reverse)
                .unwrap();
            if page.is_empty() {
                break;
            }
            cursor = page.last().unwrap().id;
            ids.extend(page.into_iter().map(|job| job.id));
        }
    }
    assert_eq!(ascending.len(), 205);
    assert!(ascending.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        descending,
        ascending.iter().rev().copied().collect::<Vec<_>>()
    );
    let cursor = descending[99];
    let mut late = asset();
    late.source_id = "inserted-between-pages".into();
    sender
        .enqueue(
            "receiver-1",
            late,
            BTreeMap::from([(digest(b"0123456789"), "resource".into())]),
        )
        .unwrap();
    assert_eq!(
        sender
            .list_filtered_ordered(cursor, 200, Some("receiver-1"), Some("queued"), true)
            .unwrap()
            .into_iter()
            .map(|job| job.id)
            .collect::<Vec<_>>(),
        descending[100..]
    );
    let running = sender.claim("receiver-2", 100).unwrap().unwrap();
    sender
        .fail(&running.attempt(), Failure::Network, 100)
        .unwrap();
    assert_eq!(
        sender.list_filtered(0, 200, None, Some("waiting")).unwrap()[0].id,
        other.id
    );
    assert!(sender
        .list_filtered(0, 200, Some("receiver-1"), Some("waiting"))
        .unwrap()
        .is_empty());
}

#[test]
fn verified_connection_recovers_only_its_network_errors_without_resuming_pause() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let mut cases = Vec::new();
    for (n, failure) in [
        Failure::Network,
        Failure::Authentication,
        Failure::Integrity,
        Failure::SourceUnavailable,
        Failure::LowSpace,
    ]
    .into_iter()
    .enumerate()
    {
        let mut a = asset();
        a.source_id = format!("route-{n}");
        let sources = BTreeMap::from([(a.resources[0].sha256.clone(), "fixture".into())]);
        let job = s.enqueue("receiver-1", a, sources).unwrap();
        let running = s.claim("receiver-1", 0).unwrap().unwrap();
        assert_eq!(job.id, running.id);
        s.acknowledge(&running.attempt(), &status(&running, false))
            .unwrap();
        s.fail(&running.attempt(), failure, 10000).unwrap();
        cases.push(s.job(job.id).unwrap());
    }
    let other = s
        .enqueue(
            "receiver-2",
            asset(),
            BTreeMap::from([(asset().resources[0].sha256.clone(), "fixture".into())]),
        )
        .unwrap();
    let running = s.claim("receiver-2", 0).unwrap().unwrap();
    s.fail(&running.attempt(), Failure::Authentication, 10000)
        .unwrap();
    s.pause_job(cases[0].id).unwrap();
    s.set_paused(true).unwrap();
    assert_eq!(s.recover_connection("receiver-1").unwrap(), 1);
    assert!(s.paused().unwrap());
    assert!(s.claim("receiver-1", i64::MAX).unwrap().is_none());
    assert_eq!(s.job(cases[0].id).unwrap().state, JobState::Paused);
    let auth = s.job(cases[1].id).unwrap();
    assert_eq!(auth.state, JobState::Queued);
    assert_eq!(auth.confirmed_bytes, 5);
    assert!(auth.generation > cases[1].generation);
    assert_eq!(s.job(other.id).unwrap().state, JobState::Failed);
    for old in &cases[2..] {
        assert_eq!(s.job(old.id).unwrap().state, old.state);
    }
    assert_eq!(s.recover_connection("receiver-1").unwrap(), 0);
}

#[test]
fn receiver_features_gain_cloud_audit_without_losing_bundle_upload() {
    let root = std::env::temp_dir().join(format!("backupduck-features-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    {
        let conn = rusqlite::Connection::open(root.join("sender.sqlite3")).unwrap();
        conn.execute_batch(
            "CREATE TABLE receiver_features(receiver_id TEXT PRIMARY KEY,bundle_upload INTEGER NOT NULL);
             INSERT INTO receiver_features VALUES('old',1);",
        )
        .unwrap();
    }
    let mut s = Sender::open(&root).unwrap();
    assert!(s.bundle_upload("old").unwrap());
    assert!(!s.cloud_audit("old").unwrap());
    s.set_cloud_audit("old", true).unwrap();
    s.set_cloud_audit("new", true).unwrap();
    s.set_bundle_upload("new", true).unwrap();
    assert!(s.cloud_audit("old").unwrap() && s.bundle_upload("old").unwrap());
    assert!(s.cloud_audit("new").unwrap() && s.bundle_upload("new").unwrap());
    assert!(!s.cloud_audit("absent").unwrap());
    // Learned separately; an older receiver never advertised it.
    assert!(!s.cloud_preexisting("old").unwrap());
    s.set_cloud_preexisting("old", true).unwrap();
    assert!(s.cloud_preexisting("old").unwrap() && s.cloud_audit("old").unwrap());
    assert!(!s.cloud_preexisting("new").unwrap());
    drop(s);
    let s = Sender::open(&root).unwrap();
    assert!(s.cloud_audit("old").unwrap() && s.cloud_preexisting("old").unwrap());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn copies_already_in_cloud_count_as_backed_up_and_may_lock() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let job = add(&mut s);
    let asset_id = job.asset.id().unwrap();
    let key = [(job.asset.source_id.clone(), job.asset.revision.clone())];
    let running = s.claim("receiver-1", 0).unwrap().unwrap();
    s.acknowledge(&running.attempt(), &status(&running, true))
        .unwrap();
    let hidden = vec![job.asset.source_id.clone()];
    s.observe_cloud(
        "receiver-1",
        &asset_id,
        "verified_counts_against_quota",
        None,
        1,
    )
    .unwrap();
    assert!(!s.locked_folder_jobs("receiver-1", &hidden).unwrap()[0].cloud_held());
    assert!(s
        .observe_cloud("receiver-1", &asset_id, "verified_elsewhere", None, 2)
        .unwrap());
    let summary = s.summary("receiver-1").unwrap();
    assert_eq!(
        (
            &summary["cloud_verified"],
            &summary["cloud_elsewhere"],
            &summary["cloud_quota"]
        ),
        (&0.into(), &1.into(), &0.into())
    );
    assert_eq!(
        s.source_states_with_cloud("receiver-1", &key, true)
            .unwrap()[&job.asset.source_id],
        "backed_up"
    );
    let jobs = s.locked_folder_jobs("receiver-1", &hidden).unwrap();
    assert_eq!(jobs[0].cloud.as_deref(), Some("verified_elsewhere"));
    assert!(jobs[0].cloud_held());
    for (cloud, held) in [
        (Some("verified"), true),
        (Some("pending"), false),
        (Some("missing"), false),
        (None, false),
    ] {
        let job = LockedFolderJob {
            cloud: cloud.map(Into::into),
            ..jobs[0].clone()
        };
        assert_eq!(job.cloud_held(), held, "{cloud:?}");
    }
}

#[test]
fn cloud_verdicts_mirror_onto_received_jobs() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let job = add(&mut s);
    let asset_id = job.asset.id().unwrap();
    let key = [(job.asset.source_id.clone(), job.asset.revision.clone())];
    assert!(!s
        .observe_cloud("receiver-2", &asset_id, "verified", None, 1)
        .unwrap());
    assert!(s
        .observe_cloud("receiver-1", &asset_id, "bogus", None, 1)
        .is_err());
    let running = s.claim("receiver-1", 0).unwrap().unwrap();
    s.acknowledge(&running.attempt(), &status(&running, true))
        .unwrap();
    assert_eq!(s.job(job.id).unwrap().cloud, None);
    assert!(!serde_json::to_string(&s.job(job.id).unwrap())
        .unwrap()
        .contains("\"cloud\""));
    let summary = s.summary("receiver-1").unwrap();
    assert_eq!(
        (
            &summary["cloud_verified"],
            &summary["cloud_quota"],
            &summary["cloud_missing"]
        ),
        (&0.into(), &0.into(), &0.into())
    );

    let before = s.revision().unwrap();
    assert!(s
        .observe_cloud(
            "receiver-1",
            &asset_id,
            "verified_counts_against_quota",
            Some("Pixel 9"),
            10
        )
        .unwrap());
    let quota = s.revision().unwrap();
    assert!(quota > before);
    assert_eq!(s.summary("receiver-1").unwrap()["cloud_quota"], 1);
    // Same state again: no UI refresh.
    s.observe_cloud(
        "receiver-1",
        &asset_id,
        "verified_counts_against_quota",
        None,
        20,
    )
    .unwrap();
    assert_eq!(s.revision().unwrap(), quota);
    assert_eq!(
        s.source_states_with_cloud("receiver-1", &key, true)
            .unwrap()[&job.asset.source_id],
        "received"
    );

    s.observe_cloud("receiver-1", &asset_id, "verified", None, 30)
        .unwrap();
    assert!(s.revision().unwrap() > quota);
    let summary = s.summary("receiver-1").unwrap();
    assert_eq!(
        (&summary["cloud_verified"], &summary["cloud_quota"]),
        (&1.into(), &0.into())
    );
    assert_eq!(s.summary("receiver-2").unwrap()["cloud_verified"], 0);
    assert_eq!(s.job(job.id).unwrap().cloud.as_deref(), Some("verified"));
    assert_eq!(
        s.list_filtered_ordered(0, 10, None, None, true).unwrap()[0]
            .cloud
            .as_deref(),
        Some("verified")
    );
    assert_eq!(
        s.source_states_with_cloud("receiver-1", &key, true)
            .unwrap()[&job.asset.source_id],
        "backed_up"
    );
    assert_eq!(
        s.source_states("receiver-1", &key).unwrap()[&job.asset.source_id],
        "received"
    );

    assert!(s.cloud_audit_run("receiver-1").unwrap().is_none());
    s.record_cloud_audit_run("receiver-1", 5, &serde_json::json!({"checked": 1}))
        .unwrap();
    drop(s);
    let s = Sender::open(&t.0).unwrap();
    assert_eq!(
        s.cloud_audit_run("receiver-1").unwrap(),
        Some((5, serde_json::json!({"checked": 1})))
    );
    assert_eq!(s.job(job.id).unwrap().cloud.as_deref(), Some("verified"));
}

#[test]
fn locked_folder_moves_migrate_and_count_in_summary() {
    let t = Temp::new();
    let job = {
        let mut s = Sender::open(&t.0).unwrap();
        add(&mut s)
    };
    // A database from before Locked Folder moves gains the tables on open.
    rusqlite::Connection::open(t.0.join("sender.sqlite3"))
        .unwrap()
        .execute_batch("DROP TABLE locked_folder_moves; DROP TABLE locked_folder_runs;")
        .unwrap();
    let mut s = Sender::open(&t.0).unwrap();
    let asset_id = job.asset.id().unwrap();
    let hidden = vec!["native:42".to_string(), "native:absent".to_string()];
    // Not received yet: not a candidate.
    assert!(s
        .locked_folder_jobs("receiver-1", &hidden)
        .unwrap()
        .is_empty());
    let running = s.claim("receiver-1", 0).unwrap().unwrap();
    s.acknowledge(&running.attempt(), &status(&running, true))
        .unwrap();
    s.observe_cloud("receiver-1", &asset_id, "verified", None, 1)
        .unwrap();
    assert_eq!(
        s.locked_folder_jobs("receiver-1", &hidden).unwrap(),
        vec![LockedFolderJob {
            asset_id: asset_id.clone(),
            cloud: Some("verified".into()),
            status: None,
            attempts: 0,
            dedup_key: None,
        }]
    );
    assert!(s
        .locked_folder_jobs("receiver-2", &hidden)
        .unwrap()
        .is_empty());
    assert!(s
        .locked_folder_jobs("receiver-1", &["native:other".into()])
        .unwrap()
        .is_empty());
    assert_eq!(s.summary("receiver-1").unwrap()["locked_folder_moved"], 0);
    assert!(s
        .record_locked_folder("receiver-1", &asset_id, "bogus", None, false, 2)
        .is_err());

    let before = s.revision().unwrap();
    s.record_locked_folder("receiver-1", &asset_id, "failed", Some("dedup-1"), true, 2)
        .unwrap();
    assert!(s.revision().unwrap() > before);
    // A failure without a new move request keeps the recorded dedup key.
    s.record_locked_folder("receiver-1", &asset_id, "failed", None, true, 3)
        .unwrap();
    let row = &s.locked_folder_jobs("receiver-1", &hidden).unwrap()[0];
    assert_eq!(
        (
            row.status.as_deref(),
            row.attempts,
            row.dedup_key.as_deref()
        ),
        (Some("failed"), 2, Some("dedup-1"))
    );
    assert_eq!(
        s.job(job.id).unwrap().locked_folder.as_deref(),
        Some("failed")
    );
    assert_eq!(s.summary("receiver-1").unwrap()["locked_folder_moved"], 0);

    s.record_locked_folder("receiver-1", &asset_id, "moved", None, false, 4)
        .unwrap();
    let moved_at: i64 = rusqlite::Connection::open(t.0.join("sender.sqlite3"))
        .unwrap()
        .query_row("SELECT moved_at_ms FROM locked_folder_moves", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(moved_at, 4);
    let summary = s.summary("receiver-1").unwrap();
    assert_eq!(summary["locked_folder_moved"], 1);
    assert_eq!(s.summary("receiver-2").unwrap()["locked_folder_moved"], 0);
    assert!(serde_json::to_string(&s.job(job.id).unwrap())
        .unwrap()
        .contains("\"locked_folder\":\"moved\""));

    assert!(s.locked_folder_run("receiver-1").unwrap().is_none());
    s.record_locked_folder_run("receiver-1", 5, &serde_json::json!({"moved": 1}))
        .unwrap();
    drop(s);
    let s = Sender::open(&t.0).unwrap();
    assert_eq!(
        s.locked_folder_run("receiver-1").unwrap(),
        Some((5, serde_json::json!({"moved": 1})))
    );
    assert_eq!(
        s.job(job.id).unwrap().locked_folder.as_deref(),
        Some("moved")
    );
}

#[test]
fn busy_retries_steadily_while_network_backs_off() {
    let t = Temp::new();
    let mut s = Sender::open(&t.0).unwrap();
    let id = add(&mut s).id;
    let mut now = 1000;
    let fail = |s: &mut Sender, failure: Failure, now: &mut i64| -> i64 {
        let j = s.claim("receiver-1", *now).unwrap().unwrap();
        assert_eq!(j.id, id);
        s.fail(&j.attempt(), failure, *now).unwrap();
        let job = s.job(id).unwrap();
        assert_eq!(job.state, JobState::Waiting);
        let delay = job.next_attempt_at - *now;
        *now = job.next_attempt_at;
        delay
    };
    // A long thermal hold: the wait never grows and attempts are not spent.
    for _ in 0..6 {
        let delay = fail(&mut s, Failure::Busy, &mut now);
        assert!((50..=70).contains(&delay), "busy delay {delay}");
    }
    assert_eq!(s.job(id).unwrap().attempts, 0);
    assert_eq!(s.job(id).unwrap().error_code.as_deref(), Some("busy"));
    // Network failures after the hold start from the base delay and double.
    let delays: Vec<_> = (0..6)
        .map(|_| fail(&mut s, Failure::Network, &mut now))
        .collect();
    let jitter = id.rem_euclid(7);
    assert_eq!(
        delays,
        [5, 10, 20, 40, 80, 160].map(|d| d + jitter).to_vec()
    );
}
