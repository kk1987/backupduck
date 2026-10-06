//! Opt-in deletion of gallery copies the cloud auditor verified. The host
//! verifies and deletes MediaStore rows; settings are re-read under the
//! receiver/settings locks at every command, so turning it off stops at once.
use super::*;
use backupduck_store::cloud::CloudReleasePolicy;
use backupduck_store::retention::GalleryCopy;

/// Bounded per sweep, like the relay sweep.
const BATCH: u32 = 4;
/// Published assets whose originals one sweep may release early. A sweep
/// runs once per host release cycle, which can take minutes while the host
/// re-hashes large gallery videos, so catch up in sizeable steps.
const BACKLOG_BATCH: u32 = 256;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn policy(settings: &maintenance::Settings) -> CloudReleasePolicy {
    CloudReleasePolicy {
        enabled: settings.cloud_release,
        now_ms: now_ms(),
        grace_ms: settings.cloud_release_grace_ms,
    }
}

fn missing_store(root: Option<&Path>) -> bool {
    root.is_some_and(|r| !r.join("store/receiver.sqlite3").is_file())
}

pub(super) fn candidates(root: Option<&Path>, after: &str) -> Result<Value> {
    if missing_store(root) {
        return Ok(json!([]));
    }
    receiver_storage::with_store(root, |store, maintenance| {
        let policy = policy(&maintenance.settings);
        if !policy.enabled {
            return Ok(json!([]));
        }
        if maintenance.settings.release_originals_on_publication {
            // Catch up assets published before the setting was turned on.
            let (count, bytes) = store.release_published_originals(BACKLOG_BATCH)?;
            if count > 0 {
                maintenance.log("publication_originals_reclaimed", None, Some(bytes))?;
            }
        }
        Ok(serde_json::to_value(store.cloud_release_candidates(
            after,
            BATCH,
            policy.now_ms,
            policy.grace_ms,
        )?)?)
    })
}

pub(super) fn release(
    root: Option<&Path>,
    id: &str,
    copy: &GalleryCopy,
    missing: bool,
) -> Result<Value> {
    receiver_storage::with_store(root, |store, maintenance| {
        let bytes =
            store.release_cloud_verified(id, copy, missing, policy(&maintenance.settings))?;
        maintenance.log("cloud_originals_released", None, Some(bytes))?;
        Ok(json!({"bytes":bytes}))
    })
}

pub(super) fn mark(root: Option<&Path>, id: &str, reason: &str) -> Result<Value> {
    receiver_storage::with_store(root, |store, maintenance| {
        let updated = store.mark_gallery_released(id, reason, now_ms())?;
        if updated {
            let size = store.gallery_copy(id)?.map(|c| c.size);
            maintenance.log(
                if reason == "missing" {
                    "cloud_gallery_missing"
                } else {
                    "cloud_gallery_released"
                },
                None,
                size,
            )?;
        }
        Ok(json!({"updated":updated}))
    })
}

/// Superseded copies (see `Receiver::superseded_candidates`), gated by the
/// same setting as cloud release.
pub(super) fn superseded_candidates(root: Option<&Path>, after: &str) -> Result<Value> {
    if missing_store(root) {
        return Ok(json!([]));
    }
    receiver_storage::with_store(root, |store, maintenance| {
        if !maintenance.settings.cloud_release {
            return Ok(json!([]));
        }
        Ok(serde_json::to_value(
            store.superseded_candidates(after, BATCH)?,
        )?)
    })
}

pub(super) fn release_superseded(
    root: Option<&Path>,
    id: &str,
    copy: &GalleryCopy,
    missing: bool,
) -> Result<Value> {
    receiver_storage::with_store(root, |store, maintenance| {
        if !maintenance.settings.cloud_release {
            return Err(Error::Conflict("cloud release disabled".into()));
        }
        let bytes = store.release_superseded(id, copy, missing)?;
        maintenance.log("superseded_originals_released", None, Some(bytes))?;
        Ok(json!({"bytes":bytes}))
    })
}

pub(super) fn mark_superseded(root: Option<&Path>, id: &str) -> Result<Value> {
    receiver_storage::with_store(root, |store, maintenance| {
        let updated = store.mark_superseded_released(id, now_ms())?;
        if updated {
            let size = store.gallery_copy(id)?.map(|c| c.size);
            maintenance.log("superseded_gallery_released", None, size)?;
        }
        Ok(json!({"updated":updated}))
    })
}
