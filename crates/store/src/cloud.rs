//! Cloud verification state for published gallery copies. Verdicts come from an
//! external auditor that looks copies up by SHA-1. Nothing here deletes media.
use super::*;
use retention::GalleryCopy;

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
}
