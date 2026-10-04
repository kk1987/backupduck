//! Cloud verification state for published gallery copies. Verdicts come from an
//! external auditor that looks copies up by SHA-1. Only the opt-in cloud
//! release below acts on them, and only for `verified` copies after a grace.
use super::*;
use retention::GalleryCopy;
use serde::Serialize;

/// Release reasons recorded for a deleted gallery copy.
pub const GALLERY_RELEASE_REASONS: [&str; 2] = ["cloud", "missing"];

/// A `verified` copy whose grace has passed and whose gallery copy is still
/// recorded as present. Originals may already be gone through relay/archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CloudReleaseCandidate {
    pub id: String,
    pub copy: GalleryCopy,
    pub originals_released: bool,
}

/// Settings snapshot taken under the receiver/settings locks at each command.
#[derive(Clone, Copy, Debug)]
pub struct CloudReleasePolicy {
    pub enabled: bool,
    pub now_ms: i64,
    pub grace_ms: u64,
}
impl CloudReleasePolicy {
    fn cutoff(self) -> i64 {
        self.now_ms
            .saturating_sub(i64::try_from(self.grace_ms).unwrap_or(i64::MAX))
    }
}

/// Never `verified_counts_against_quota` or `missing`: only free cloud copies.
const CLOUD_RELEASE_ELIGIBLE: &str =
    "a.received=1 AND a.processing='complete' AND a.cloud_state='verified' \
     AND a.gallery_released=0 AND a.cloud_checked_at_ms IS NOT NULL AND a.cloud_checked_at_ms<=?2";

/// A copy is first looked up ten minutes after publication.
const FIRST_CHECK_MS: i64 = 600_000;
/// Repeated lookups back off exponentially up to one day.
const MAX_BACKOFF_MS: i64 = 86_400_000;

impl Receiver {
    /// Published copies with SHA-1 evidence. `all` ignores state and timing;
    /// otherwise only copies whose next lookup is due are listed.
    pub fn cloud_due(
        &self,
        after: &str,
        limit: u32,
        now_ms: i64,
        all: bool,
    ) -> Result<CloudDuePage> {
        if !after.is_empty() && !valid_digest(after) {
            return Err(Error::Invalid("cloud cursor".into()));
        }
        if limit == 0 || limit as usize > MAX_CLOUD_ITEMS {
            return Err(Error::Invalid("cloud limit".into()));
        }
        let mut query = self
            .conn
            .prepare(
                "SELECT a.id,g.copy,json_extract(a.manifest,'$.kind'),a.published_at_ms,a.cloud_state,a.cloud_checks \
                 FROM assets a JOIN gallery_copies g ON g.asset_id=a.id \
                 WHERE a.received=1 AND a.processing='complete' AND json_extract(g.copy,'$.sha1') IS NOT NULL AND a.id>?1 \
                 AND (?4 OR (a.cloud_state IN ('pending','verified_counts_against_quota') AND a.published_at_ms<=?3-?5 \
                 AND (a.cloud_checked_at_ms IS NULL OR a.cloud_checked_at_ms<=?3-MIN(?5*(1<<MIN(a.cloud_checks,8)),?6)))) \
                 ORDER BY a.id LIMIT ?2",
            )
            .map_err(db)?;
        let rows = query
            .query_map(
                params![after, limit, now_ms, all, FIRST_CHECK_MS, MAX_BACKOFF_MS],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, u32>(5)?,
                    ))
                },
            )
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        let items = rows
            .into_iter()
            .map(
                |(asset_id, copy, kind, published_at_ms, cloud_state, cloud_checks)| {
                    let copy: GalleryCopy = serde_json::from_str(&copy)?;
                    Ok(CloudDueItem {
                        asset_id,
                        sha1: copy.sha1.ok_or(Error::Integrity)?,
                        sha256: copy.sha256,
                        size: copy.size,
                        display_name: copy.display_name,
                        kind,
                        published_at_ms,
                        cloud_state,
                        cloud_checks,
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;
        let next = (items.len() == limit as usize)
            .then(|| items.last().map(|i| i.asset_id.clone()))
            .flatten();
        Ok(CloudDuePage { items, next })
    }

    /// Record auditor verdicts. A verdict for different bytes than the stored
    /// copy is stale and rejected; `verified` never regresses.
    pub fn observe_cloud(
        &mut self,
        observations: &[CloudObservation],
        now_ms: i64,
    ) -> Result<CloudSummary> {
        if observations.len() > MAX_CLOUD_ITEMS {
            return Err(Error::Invalid("too many cloud observations".into()));
        }
        observations
            .iter()
            .try_for_each(CloudObservation::validate)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut summary = CloudSummary::default();
        for o in observations {
            let row: Option<(String, String, Option<i64>, Option<String>)> = tx
                .query_row(
                    "SELECT a.processing,a.cloud_state,a.published_at_ms,g.copy FROM assets a LEFT JOIN gallery_copies g ON g.asset_id=a.id WHERE a.id=?1",
                    [&o.asset_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(db)?;
            let Some((processing, state, published_at_ms, copy)) = row else {
                summary.rejected += 1;
                continue;
            };
            let stored_sha1 = copy
                .and_then(|c| serde_json::from_str::<GalleryCopy>(&c).ok())
                .and_then(|c| c.sha1);
            if processing != "complete" || stored_sha1.as_deref() != Some(o.sha1.as_str()) {
                summary.rejected += 1;
                continue;
            }
            let next = next_cloud_state(&state, o.result, published_at_ms, now_ms);
            if state != "verified" || o.result == CloudResult::Free {
                tx.execute(
                    "UPDATE assets SET cloud_state=?2,cloud_checks=cloud_checks+1,cloud_checked_at_ms=?3,\
                     cloud_media_key=COALESCE(?4,cloud_media_key),cloud_device_model=COALESCE(?5,cloud_device_model) WHERE id=?1",
                    params![o.asset_id, next, now_ms, o.media_key, o.device_model],
                )
                .map_err(db)?;
            }
            match next {
                "verified" => summary.verified += 1,
                "verified_counts_against_quota" => summary.quota += 1,
                "missing" => summary.missing += 1,
                _ => summary.still_pending += 1,
            }
        }
        tx.commit().map_err(db)?;
        Ok(summary)
    }

    /// Bounded page of copies the cloud release may delete, in id order.
    pub fn cloud_release_candidates(
        &self,
        after: &str,
        limit: u32,
        now_ms: i64,
        grace_ms: u64,
    ) -> Result<Vec<CloudReleaseCandidate>> {
        if !after.is_empty() && !valid_digest(after) {
            return Err(Error::Invalid("cloud cursor".into()));
        }
        if limit == 0 || limit as usize > MAX_CLOUD_ITEMS {
            return Err(Error::Invalid("cloud limit".into()));
        }
        let policy = CloudReleasePolicy {
            enabled: true,
            now_ms,
            grace_ms,
        };
        let mut query = self
            .conn
            .prepare(&format!(
                "SELECT a.id,g.copy,a.originals_released FROM assets a JOIN gallery_copies g ON g.asset_id=a.id \
                 WHERE a.id>?1 AND {CLOUD_RELEASE_ELIGIBLE} ORDER BY a.id LIMIT ?3"
            ))
            .map_err(db)?;
        let rows = query
            .query_map(params![after, policy.cutoff(), limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        rows.into_iter()
            .map(|(id, copy, originals_released)| {
                Ok(CloudReleaseCandidate {
                    id,
                    copy: serde_json::from_str(&copy)?,
                    originals_released,
                })
            })
            .collect()
    }

    /// Release originals of an eligible copy before the host deletes the
    /// gallery copy. `fresh` must be a fresh read matching the stored evidence,
    /// or, with `missing`, the stored evidence itself for a copy the host found
    /// gone: the cloud already matched its SHA-1. The durable marker precedes
    /// blob deletion; repeating it is harmless. Returns bytes freed.
    pub fn release_cloud_verified(
        &mut self,
        id: &str,
        fresh: &GalleryCopy,
        missing: bool,
        policy: CloudReleasePolicy,
    ) -> Result<u64> {
        if !policy.enabled {
            return Err(Error::Conflict("cloud release disabled".into()));
        }
        fresh.validate()?;
        let stored = self.gallery_copy(id)?.ok_or(Error::Integrity)?;
        let proven = if missing {
            stored == *fresh
        } else {
            fresh.sha1.is_some() && stored.matches(fresh)
        };
        if !proven {
            return Err(Error::Integrity);
        }
        if !self.cloud_release_eligible(id, policy)? {
            return Err(Error::Conflict("cloud release not eligible".into()));
        }
        self.conn
            .execute(
                "UPDATE assets SET originals_released=1,release_reason='cloud' WHERE id=?1 AND originals_released=0",
                [id],
            )
            .map_err(db)?;
        self.reclaim_unreferenced()
    }

    /// Record that the host deleted (`cloud`) or found gone (`missing`) the
    /// gallery copy after `release_cloud_verified`. Not gated by the setting:
    /// it records what already happened. Returns whether the row changed.
    pub fn mark_gallery_released(&mut self, id: &str, reason: &str, now_ms: i64) -> Result<bool> {
        if !GALLERY_RELEASE_REASONS.contains(&reason) {
            return Err(Error::Invalid("gallery release reason".into()));
        }
        let (originals_released, state, released): (bool, String, bool) = self
            .conn
            .query_row(
                "SELECT originals_released,cloud_state,gallery_released FROM assets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db)?
            .ok_or(Error::NotFound)?;
        if released {
            return Ok(false);
        }
        if !originals_released || state != "verified" {
            return Err(Error::Conflict("gallery release not prepared".into()));
        }
        self.conn
            .execute(
                "UPDATE assets SET gallery_released=1,gallery_released_at_ms=?2,gallery_release_reason=?3 WHERE id=?1 AND gallery_released=0",
                params![id, now_ms, reason],
            )
            .map_err(db)?;
        Ok(true)
    }

    fn cloud_release_eligible(&self, id: &str, policy: CloudReleasePolicy) -> Result<bool> {
        self.conn
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM assets a WHERE a.id=?1 AND {CLOUD_RELEASE_ELIGIBLE})"),
                params![id, policy.cutoff()],
                |r| r.get(0),
            )
            .map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "backupduck-cloud-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn received(r: &mut Receiver, name: &str) -> String {
        let bytes = name.as_bytes();
        let asset = Asset {
            version: PROTOCOL_VERSION,
            source_id: name.into(),
            revision: "1".into(),
            kind: AssetKind::Photo,
            metadata: Default::default(),
            resources: vec![Resource {
                role: ResourceRole::Photo,
                filename: "photo.jpg".into(),
                media_type: "image/jpeg".into(),
                size: bytes.len() as u64,
                sha256: digest(bytes),
            }],
        };
        let id = r.register(asset).unwrap().asset_id;
        r.append(&id, &digest(bytes), 0, bytes, &digest(bytes))
            .unwrap();
        r.commit(&id).unwrap();
        id
    }
    fn copy(n: u8, sha1: Option<char>) -> GalleryCopy {
        GalleryCopy {
            locator: format!("content://media/external_primary/images/media/{n}"),
            sha256: digest(&[n]),
            size: 1,
            display_name: Some(format!("IMG_{n}.JPG")),
            sha1: sha1.map(|c| c.to_string().repeat(40)),
        }
    }
    fn observe(id: &str, sha1: char, result: CloudResult) -> CloudObservation {
        CloudObservation {
            asset_id: id.into(),
            sha1: sha1.to_string().repeat(40),
            result,
            media_key: Some("key-1".into()),
            device_model: Some("Pixel 9".into()),
        }
    }
    fn state(r: &Receiver, id: &str) -> String {
        r.status(id).unwrap().cloud_state.unwrap()
    }

    #[test]
    fn missing_sha1_is_a_wildcard_only_in_stored_evidence() {
        let old = copy(1, None);
        let fresh = copy(1, Some('a'));
        assert!(old.matches(&fresh));
        assert!(fresh.matches(&fresh));
        assert!(!fresh.matches(&old));
        assert!(!fresh.matches(&copy(1, Some('b'))));
        assert!(!old.matches(&copy(2, Some('a'))));
        let mut bad = fresh.clone();
        bad.sha1 = Some("A".repeat(40));
        assert!(bad.validate().is_err());
        assert!(!serde_json::to_string(&old).unwrap().contains("sha1"));
    }

    #[test]
    fn pre_cloud_database_opens_and_catalog_reads_it() {
        let t = Temp::new();
        {
            let conn = Connection::open(t.0.join("receiver.sqlite3")).unwrap();
            conn.execute_batch(
                "CREATE TABLE assets(id TEXT PRIMARY KEY, manifest TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, processing TEXT NOT NULL DEFAULT 'not_requested', originals_released INTEGER NOT NULL DEFAULT 0, release_reason TEXT, processing_error TEXT, received_at_ms INTEGER, published_at_ms INTEGER);
                 CREATE TABLE blobs(hash TEXT PRIMARY KEY, size INTEGER NOT NULL, ready INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
        }
        let page = catalog::Catalog::open(&t.0)
            .unwrap()
            .page(None, "all", "all", 10)
            .unwrap();
        assert!(page.items.is_empty());
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "legacy");
        assert_eq!(state(&r, &id), "unknown");
        assert_eq!(r.overview().unwrap()["cloud_verified"], 0);
        drop(r);
        // Reopening must not try to add the columns again.
        let r = Receiver::open(&t.0, 1 << 20).unwrap();
        let item = &catalog::Catalog::open(&t.0)
            .unwrap()
            .page(None, "all", "all", 10)
            .unwrap()
            .items[0];
        assert_eq!(item.cloud_state, "unknown");
        assert!(!item.gallery_released);
        drop(r);
    }

    #[test]
    fn manual_relay_inspection_never_sees_backfill_rows() {
        // RC.3's manual historical inspection treats every candidate as a
        // release; SHA-1 backfill rows must stay out of that list.
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "released");
        r.record_gallery_copy(&id, &copy(1, None)).unwrap();
        r.release_gallery_copy(&id, &copy(1, None), true).unwrap();
        assert_eq!(state(&r, &id), "unknown");
        let ids = |v: Vec<retention::GalleryCandidate>| -> Vec<String> {
            v.into_iter().map(|c| c.publication.id).collect()
        };
        assert_eq!(
            ids(r
                .gallery_candidates_with_backfill("", false, false)
                .unwrap()),
            vec![id.clone()]
        );
        assert!(r
            .gallery_candidates_with_backfill("", false, true)
            .unwrap()
            .is_empty());
        assert!(r.gallery_candidates_scoped("", true).unwrap().is_empty());
        assert!(r.gallery_candidates("").unwrap().is_empty());
    }

    #[test]
    fn sha1_backfill_is_idempotent_and_queues_the_copy() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "backfill");
        r.record_gallery_copy(&id, &copy(1, None)).unwrap();
        assert_eq!(state(&r, &id), "unknown");
        let candidates = r
            .gallery_candidates_with_backfill("", false, false)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].confirmed);
        assert!(r
            .cloud_due("", 100, i64::MAX / 2, true)
            .unwrap()
            .items
            .is_empty());
        assert!(matches!(
            r.backfill_gallery_sha1(&id, &copy(1, None)),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            r.backfill_gallery_sha1(&id, &copy(2, Some('a'))),
            Err(Error::Integrity)
        ));
        assert!(r.backfill_gallery_sha1(&id, &copy(1, Some('a'))).unwrap());
        assert!(!r.backfill_gallery_sha1(&id, &copy(1, Some('a'))).unwrap());
        assert!(matches!(
            r.backfill_gallery_sha1(&id, &copy(1, Some('b'))),
            Err(Error::Integrity)
        ));
        assert_eq!(r.gallery_copy(&id).unwrap(), Some(copy(1, Some('a'))));
        assert_eq!(state(&r, &id), "pending");
        assert!(r
            .gallery_candidates_with_backfill("", false, false)
            .unwrap()
            .is_empty());
        // Relay keeps its own candidates; stored SHA-1 now gates fresh proofs.
        assert_eq!(
            r.gallery_candidates_with_backfill("", true, false)
                .unwrap()
                .len(),
            1
        );
        assert!(r.release_gallery_copy(&id, &copy(1, None), true).is_err());
        r.record_gallery_copy(&id, &copy(1, Some('a'))).unwrap();
    }

    #[test]
    fn record_upgrades_older_evidence_and_prepare_accepts_new_sha1() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "upgrade");
        r.prepare_gallery_copy(&id, &copy(1, None)).unwrap();
        r.record_gallery_copy(&id, &copy(1, Some('c'))).unwrap();
        assert_eq!(r.gallery_copy(&id).unwrap(), Some(copy(1, Some('c'))));
        assert_eq!(state(&r, &id), "pending");
        assert!(r.prepare_gallery_copy(&id, &copy(1, Some('d'))).is_err());
        assert!(r.record_gallery_copy(&id, &copy(1, Some('d'))).is_err());
    }

    #[test]
    fn due_listing_waits_ten_minutes_and_backs_off() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "due");
        let other = received(&mut r, "other");
        r.record_gallery_copy(&id, &copy(1, Some('a'))).unwrap();
        r.record_gallery_copy(&other, &copy(2, None)).unwrap();
        assert!(r.cloud_due("bad", 10, 0, true).is_err());
        assert!(r.cloud_due("", 0, 0, true).is_err());
        assert!(r.cloud_due("", 101, 0, true).is_err());
        let all = r.cloud_due("", 100, 0, true).unwrap();
        assert_eq!(all.items.len(), 1);
        assert_eq!(all.next, None);
        let item = &all.items[0];
        assert_eq!(
            (item.sha1.as_str(), item.kind.as_str(), item.cloud_checks),
            ("a".repeat(40).as_str(), "photo", 0)
        );
        assert_eq!(item.display_name.as_deref(), Some("IMG_1.JPG"));
        let published = item.published_at_ms.unwrap();
        let minute = 60_000;
        let due = |r: &Receiver, now: i64| r.cloud_due("", 100, now, false).unwrap().items.len();
        assert_eq!(due(&r, published + 9 * minute), 0);
        assert_eq!(due(&r, published + 10 * minute), 1);
        let checked = published + 10 * minute;
        r.observe_cloud(&[observe(&id, 'a', CloudResult::NotFound)], checked)
            .unwrap();
        assert_eq!(due(&r, checked + 19 * minute), 0);
        assert_eq!(due(&r, checked + 20 * minute), 1);
        let checked = checked + 20 * minute;
        r.observe_cloud(&[observe(&id, 'a', CloudResult::NotFound)], checked)
            .unwrap();
        assert_eq!(due(&r, checked + 39 * minute), 0);
        assert_eq!(due(&r, checked + 40 * minute), 1);
        r.conn
            .execute("UPDATE assets SET cloud_checks=30 WHERE id=?1", [&id])
            .unwrap();
        assert_eq!(due(&r, checked + 24 * 60 * minute - 1), 0);
        assert_eq!(due(&r, checked + 24 * 60 * minute), 1);
        // Pagination returns the last id only for a full page.
        let page = r.cloud_due("", 1, 0, true).unwrap();
        assert_eq!(page.next.as_deref(), Some(id.as_str()));
        assert!(r.cloud_due(&id, 1, 0, true).unwrap().items.is_empty());
    }

    #[test]
    fn observations_move_states_and_reject_stale_verdicts() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let id = received(&mut r, "observe");
        let late = received(&mut r, "late");
        let unpublished = received(&mut r, "unpublished");
        r.record_gallery_copy(&id, &copy(1, Some('a'))).unwrap();
        r.record_gallery_copy(&late, &copy(2, Some('b'))).unwrap();
        let published = r.cloud_due("", 100, 0, true).unwrap().items[0]
            .published_at_ms
            .unwrap();
        let day = 86_400_000;
        let summary = r
            .observe_cloud(
                &[
                    observe(&id, 'f', CloudResult::Free),
                    observe(&unpublished, 'a', CloudResult::Free),
                    observe(&digest(b"absent"), 'a', CloudResult::Free),
                    observe(&id, 'a', CloudResult::NotFound),
                    observe(&late, 'b', CloudResult::NotFound),
                ],
                published + day,
            )
            .unwrap();
        assert_eq!(
            summary,
            CloudSummary {
                still_pending: 2,
                rejected: 3,
                ..Default::default()
            }
        );
        let summary = r
            .observe_cloud(
                &[observe(&late, 'b', CloudResult::NotFound)],
                published + 8 * day,
            )
            .unwrap();
        assert_eq!(summary.missing, 1);
        assert_eq!(state(&r, &late), "missing");
        r.observe_cloud(
            &[observe(&id, 'a', CloudResult::CountsAgainstQuota)],
            published + day,
        )
        .unwrap();
        assert_eq!(state(&r, &id), "verified_counts_against_quota");
        let summary = r
            .observe_cloud(
                &[
                    observe(&id, 'a', CloudResult::Free),
                    observe(&late, 'b', CloudResult::Free),
                ],
                published + 9 * day,
            )
            .unwrap();
        assert_eq!(summary.verified, 2);
        let summary = r
            .observe_cloud(
                &[
                    observe(&id, 'a', CloudResult::NotFound),
                    observe(&late, 'b', CloudResult::CountsAgainstQuota),
                ],
                published + 30 * day,
            )
            .unwrap();
        assert_eq!(summary.verified, 2);
        assert_eq!(
            (state(&r, &id), state(&r, &late)),
            ("verified".into(), "verified".into())
        );
        let (key, model, checks): (String, String, i64) = r
            .conn
            .query_row(
                "SELECT cloud_media_key,cloud_device_model,cloud_checks FROM assets WHERE id=?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (key.as_str(), model.as_str(), checks),
            ("key-1", "Pixel 9", 3)
        );
        assert!(r
            .cloud_due("", 100, published + 30 * day, false)
            .unwrap()
            .items
            .is_empty());
        let overview = r.overview().unwrap();
        assert_eq!(overview["cloud_verified"], 2);
        assert_eq!(overview["cloud_missing"], 0);
        let mut bad = observe(&id, 'a', CloudResult::Free);
        bad.device_model = Some("x".repeat(65));
        assert!(r.observe_cloud(&[bad], 0).is_err());
        let many = vec![observe(&id, 'a', CloudResult::Free); 101];
        assert!(r.observe_cloud(&many, 0).is_err());
    }

    fn policy(enabled: bool, now_ms: i64) -> CloudReleasePolicy {
        CloudReleasePolicy {
            enabled,
            now_ms,
            grace_ms: HOUR,
        }
    }
    const HOUR: u64 = 3_600_000;
    /// A published copy (SHA-1 'a'+n) with a cloud verdict recorded at `at`.
    fn observed(r: &mut Receiver, name: &str, n: u8, result: CloudResult, at: i64) -> String {
        let id = received(r, name);
        let sha1 = (b'a' + n) as char;
        r.record_gallery_copy(&id, &copy(n, Some(sha1))).unwrap();
        r.observe_cloud(&[observe(&id, sha1, result)], at).unwrap();
        id
    }
    fn candidate_ids(r: &Receiver, now: i64) -> Vec<String> {
        r.cloud_release_candidates("", 100, now, HOUR)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn cloud_release_lists_only_free_verified_copies_after_grace() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let at = 1_000_000_000;
        let verified = observed(&mut r, "verified", 1, CloudResult::Free, at);
        let quota = observed(&mut r, "quota", 2, CloudResult::CountsAgainstQuota, at);
        let pending = observed(&mut r, "pending", 3, CloudResult::NotFound, at);
        let unknown = received(&mut r, "unknown");
        r.record_gallery_copy(&unknown, &copy(4, None)).unwrap();
        let missing = observed(&mut r, "missing", 5, CloudResult::NotFound, i64::MAX / 2);
        assert_eq!(state(&r, &quota), "verified_counts_against_quota");
        assert_eq!(state(&r, &pending), "pending");
        assert_eq!(state(&r, &missing), "missing");
        assert!(candidate_ids(&r, at + HOUR as i64 - 1).is_empty());
        assert_eq!(candidate_ids(&r, at + HOUR as i64), vec![verified.clone()]);
        assert!(candidate_ids(&r, i64::MAX).contains(&verified));
        assert_eq!(candidate_ids(&r, i64::MAX).len(), 1);
        let page = r
            .cloud_release_candidates("", 1, at + HOUR as i64, HOUR)
            .unwrap();
        assert_eq!(page[0].copy, copy(1, Some('b')));
        assert!(!page[0].originals_released);
        assert!(r
            .cloud_release_candidates(&verified, 1, at + HOUR as i64, HOUR)
            .unwrap()
            .is_empty());
        assert!(r.cloud_release_candidates("bad", 1, 0, HOUR).is_err());
        assert!(r.cloud_release_candidates("", 0, 0, HOUR).is_err());
        assert!(r.cloud_release_candidates("", 101, 0, HOUR).is_err());
    }

    #[test]
    fn cloud_release_requires_setting_grace_and_matching_proof() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let at = 1_000_000_000;
        let id = observed(&mut r, "release", 1, CloudResult::Free, at);
        let quota = observed(&mut r, "quota", 2, CloudResult::CountsAgainstQuota, at);
        let fresh = copy(1, Some('b'));
        let later = at + HOUR as i64;
        let blobs = |r: &Receiver| -> i64 {
            r.conn
                .query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(blobs(&r), 2);
        assert!(matches!(
            r.release_cloud_verified(&id, &fresh, false, policy(false, later)),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            r.release_cloud_verified(&id, &fresh, false, policy(true, later - 1)),
            Err(Error::Conflict(_))
        ));
        // Missing or different SHA-1, other bytes, other locator.
        for bad in [copy(1, None), copy(1, Some('c')), copy(2, Some('b'))] {
            assert!(matches!(
                r.release_cloud_verified(&id, &bad, false, policy(true, later)),
                Err(Error::Integrity)
            ));
        }
        assert!(r
            .release_cloud_verified(&quota, &copy(2, Some('c')), false, policy(true, later))
            .is_err());
        assert!(r.mark_gallery_released(&id, "cloud", later).is_err());
        assert!(!r.originals_released(&id).unwrap());
        assert_eq!(
            r.release_cloud_verified(&id, &fresh, false, policy(true, later))
                .unwrap(),
            "release".len() as u64
        );
        assert!(r.originals_released(&id).unwrap());
        assert_eq!(blobs(&r), 1);
        // A crash before the gallery mark repeats the release harmlessly.
        assert_eq!(candidate_ids(&r, later), vec![id.clone()]);
        assert!(r.cloud_release_candidates("", 1, later, HOUR).unwrap()[0].originals_released);
        assert_eq!(
            r.release_cloud_verified(&id, &fresh, false, policy(true, later))
                .unwrap(),
            0
        );
        assert!(r.mark_gallery_released(&id, "elsewhere", later).is_err());
        assert!(r.mark_gallery_released(&id, "cloud", later).unwrap());
        assert!(!r.mark_gallery_released(&id, "cloud", later).unwrap());
        assert!(candidate_ids(&r, later).is_empty());
        assert!(r
            .release_cloud_verified(&id, &fresh, false, policy(true, later))
            .is_err());
        let (reason, released_at, release_reason): (String, i64, String) = r
            .conn
            .query_row(
                "SELECT gallery_release_reason,gallery_released_at_ms,release_reason FROM assets WHERE id=?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (reason.as_str(), released_at, release_reason.as_str()),
            ("cloud", later, "cloud")
        );
        assert_eq!(r.overview().unwrap()["gallery_released"], 1);
        let counts = catalog::Catalog::open(&t.0).unwrap().counts().unwrap();
        assert_eq!(
            (
                &counts["cloud_verified"],
                &counts["gallery_released"],
                &counts["gallery_released_bytes"]
            ),
            (
                &serde_json::json!(1),
                &serde_json::json!(1),
                &serde_json::json!(1)
            )
        );
        let item = catalog::Catalog::open(&t.0)
            .unwrap()
            .page(None, "all", "all", 10)
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.id == id)
            .unwrap();
        assert!(item.gallery_released && item.originals_released);
        assert_eq!(item.cloud_state, "verified");
    }

    /// A Mac sender may move a verified item into the Google Photos Locked
    /// Folder, where SHA-1 lookups no longer find it. The audit must never
    /// list a verified row again, and its SHA-1 stays available to `all`
    /// listings even after the gallery copy is released.
    #[test]
    fn verified_rows_are_never_due_but_keep_their_sha1() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let at = 1_000_000_000;
        let id = observed(&mut r, "verified", 1, CloudResult::Free, at);
        let due = |r: &Receiver, now: i64| r.cloud_due("", 100, now, false).unwrap().items.len();
        for now in [at, at + 86_400_000, at + 365 * 86_400_000, i64::MAX / 2] {
            assert_eq!(due(&r, now), 0, "{now}");
        }
        // A stray not-found verdict does not regress it into the due list.
        r.observe_cloud(&[observe(&id, 'b', CloudResult::NotFound)], at + 1)
            .unwrap();
        assert_eq!(state(&r, &id), "verified");
        assert_eq!(due(&r, i64::MAX / 2), 0);
        let later = at + HOUR as i64;
        r.release_cloud_verified(&id, &copy(1, Some('b')), false, policy(true, later))
            .unwrap();
        assert!(r.mark_gallery_released(&id, "cloud", later).unwrap());
        assert_eq!(due(&r, i64::MAX / 2), 0);
        let all = r.cloud_due("", 100, 0, true).unwrap();
        assert_eq!(all.items.len(), 1);
        assert_eq!(all.items[0].sha1, "b".repeat(40));
        assert_eq!(all.items[0].cloud_state, "verified");
    }

    #[test]
    fn relay_released_rows_only_mark_the_gallery_and_missing_needs_stored_evidence() {
        let t = Temp::new();
        let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
        let at = 1_000_000_000;
        let later = at + HOUR as i64;
        let relayed = observed(&mut r, "relayed", 1, CloudResult::Free, at);
        r.release_gallery_copy(&relayed, &copy(1, Some('b')), true)
            .unwrap();
        assert_eq!(candidate_ids(&r, later), vec![relayed.clone()]);
        assert_eq!(
            r.release_cloud_verified(&relayed, &copy(1, Some('b')), false, policy(true, later))
                .unwrap(),
            0
        );
        let reason: String = r
            .conn
            .query_row(
                "SELECT release_reason FROM assets WHERE id=?1",
                [&relayed],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reason, "gallery");
        assert!(r.mark_gallery_released(&relayed, "cloud", later).unwrap());

        // The copy vanished: the stored evidence stands in for a fresh read.
        let gone = observed(&mut r, "gone", 2, CloudResult::Free, at);
        let mut other = copy(2, Some('d'));
        assert!(matches!(
            r.release_cloud_verified(&gone, &other, true, policy(true, later)),
            Err(Error::Integrity)
        ));
        other.sha1 = None;
        assert!(r
            .release_cloud_verified(&gone, &other, true, policy(true, later))
            .is_err());
        assert!(r
            .release_cloud_verified(&gone, &copy(2, Some('c')), true, policy(true, later - 1))
            .is_err());
        assert_eq!(
            r.release_cloud_verified(&gone, &copy(2, Some('c')), true, policy(true, later))
                .unwrap(),
            "gone".len() as u64
        );
        assert!(r.mark_gallery_released(&gone, "missing", later).unwrap());
        assert!(candidate_ids(&r, later).is_empty());
        // Without stored evidence there is nothing to stand in.
        let bare = received(&mut r, "bare");
        assert!(r
            .release_cloud_verified(&bare, &copy(3, Some('d')), true, policy(true, later))
            .is_err());
        assert!(matches!(
            r.mark_gallery_released(&digest(b"absent"), "cloud", later),
            Err(Error::NotFound)
        ));
    }
}
