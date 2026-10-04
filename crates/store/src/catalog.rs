//! Read-only, host-local history. Browsing never verifies, renames or deletes blobs.
use backupduck_core::{Asset, Error, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct HistoryItem {
    pub cursor: i64,
    pub id: String,
    pub filename: String,
    pub kind: backupduck_core::AssetKind,
    pub burst_primary: Option<bool>,
    pub total_bytes: u64,
    pub confirmed_bytes: u64,
    pub receipt: &'static str,
    pub processing: String,
    pub processing_error: Option<String>,
    pub captured_at_ms: Option<i64>,
    pub received_at_ms: Option<i64>,
    pub published_at_ms: Option<i64>,
    pub originals_released: bool,
    pub release_reason: Option<String>,
    pub cloud_state: String,
    pub gallery_released: bool,
    pub senders: Vec<super::devices::Peer>,
}
#[derive(Serialize)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub total: i64,
    pub next_cursor: Option<i64>,
}
pub struct Catalog {
    root: PathBuf,
    conn: Option<Connection>,
}
impl Catalog {
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("receiver.sqlite3");
        let conn = if path.exists() {
            let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(super::db)?;
            conn.busy_timeout(std::time::Duration::from_millis(250))
                .map_err(super::db)?;
            Some(conn)
        } else {
            None
        };
        Ok(Self {
            root: root.into(),
            conn,
        })
    }
    pub fn counts(&self) -> Result<serde_json::Value> {
        let Some(conn) = &self.conn else {
            return Ok(
                serde_json::json!({"total":0,"received":0,"published":0,"cloud_verified":0,"gallery_released":0,"gallery_released_bytes":0}),
            );
        };
        let (total, received, published): (i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(received),0),COALESCE(SUM(processing='complete'),0) FROM assets", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(super::db)?;
        let has_cloud: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='cloud_state')",
                [],
                |r| r.get(0),
            )
            .map_err(super::db)?;
        // Copies the cloud holds (release-eligible states) and gallery bytes
        // freed by cloud release, from the recorded copy sizes.
        let (verified, released, released_bytes): (i64, i64, i64) = if has_cloud {
            conn.query_row(
                "SELECT COALESCE(SUM(cloud_state IN ('verified','verified_elsewhere')),0),COALESCE(SUM(gallery_released),0),\
                 (SELECT COALESCE(SUM(json_extract(g.copy,'$.size')),0) FROM assets a JOIN gallery_copies g ON g.asset_id=a.id WHERE a.gallery_released=1) FROM assets",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(super::db)?
        } else {
            (0, 0, 0)
        };
        Ok(
            serde_json::json!({"total":total,"received":received,"published":published,"cloud_verified":verified,"gallery_released":released,"gallery_released_bytes":released_bytes}),
        )
    }
    pub fn reserved_bytes(&self) -> Result<u64> {
        let Some(conn) = &self.conn else { return Ok(0) };
        conn.query_row("SELECT COALESCE(SUM(size),0) FROM blobs", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(super::db)
        .and_then(|v| u64::try_from(v).map_err(|_| Error::Integrity))
    }
    pub fn page(
        &self,
        before: Option<i64>,
        state: &str,
        kind: &str,
        limit: u32,
    ) -> Result<HistoryPage> {
        self.page_for_sender(before, state, kind, limit, None)
    }
    pub fn page_for_sender(
        &self,
        before: Option<i64>,
        state: &str,
        kind: &str,
        limit: u32,
        sender: Option<&str>,
    ) -> Result<HistoryPage> {
        self.page_internal(before, state, kind, limit, sender, None)
    }
    /// Numbered browser pages. Filters are applied before LIMIT/OFFSET.
    pub fn numbered_page(
        &self,
        state: &str,
        kind: &str,
        page: u32,
        per_page: u32,
    ) -> Result<HistoryPage> {
        if page == 0 || page > 100_000 || ![20, 50, 100].contains(&per_page) {
            return Err(Error::Invalid("history page".into()));
        }
        let offset = i64::from(page - 1) * i64::from(per_page);
        self.page_internal(None, state, kind, per_page, None, Some(offset))
    }
    fn page_internal(
        &self,
        before: Option<i64>,
        state: &str,
        kind: &str,
        limit: u32,
        sender: Option<&str>,
        numbered_offset: Option<i64>,
    ) -> Result<HistoryPage> {
        if sender.is_some_and(|id| id != "unknown" && !backupduck_core::valid_digest(id)) {
            return Err(Error::Invalid("sender filter".into()));
        }
        if ![
            "all",
            "receiving",
            "received",
            "processing",
            "published",
            "failed",
        ]
        .contains(&state)
            || !["all", "photo", "video", "motion", "burst"].contains(&kind)
            || before.is_some_and(|v| v <= 0)
        {
            return Err(Error::Invalid("history filter".into()));
        }
        let Some(conn) = &self.conn else {
            return Ok(HistoryPage {
                items: vec![],
                total: 0,
                next_cursor: None,
            });
        };
        let has_senders: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='asset_senders')",[],|r|r.get(0)).map_err(super::db)?;
        let sender_filter = if has_senders {
            "(?5 IS NULL OR (?5='unknown' AND NOT EXISTS(SELECT 1 FROM asset_senders s WHERE s.asset_id=assets.id)) OR EXISTS(SELECT 1 FROM asset_senders s WHERE s.asset_id=assets.id AND s.sender_id=?5))"
        } else {
            "(?5 IS NULL OR ?5='unknown')"
        };
        let filter = "(?1='all' OR (?1='receiving' AND received=0) OR (?1='received' AND received=1) OR (?1='published' AND processing='complete') OR (?1='failed' AND processing='failed') OR (?1='processing' AND received=1 AND processing IN ('pending','not_requested'))) AND (?2='all' OR json_extract(manifest,'$.kind')=?2 OR (?2='burst' AND json_extract(manifest,'$.metadata.burst_group_ref') IS NOT NULL))";
        let filter = format!("({filter}) AND {sender_filter}");
        let total = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM assets WHERE {filter}"),
                params![state, kind, Option::<i64>::None, 0, sender],
                |r| r.get(0),
            )
            .map_err(super::db)?;
        let limit = limit.clamp(1, 100) as usize;
        let has_reason: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='release_reason')", [], |r| r.get(0)).map_err(super::db)?;
        let reason_column = if has_reason { "release_reason" } else { "NULL" };
        let has_error: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='processing_error')", [], |r| r.get(0)).map_err(super::db)?;
        let error_column = if has_error {
            "processing_error"
        } else {
            "NULL"
        };
        let has_received_at: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='received_at_ms')", [], |r| r.get(0)).map_err(super::db)?;
        let received_at_column = if has_received_at {
            "received_at_ms"
        } else {
            "NULL"
        };
        let has_published_at: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='published_at_ms')", [], |r| r.get(0)).map_err(super::db)?;
        let published_at_column = if has_published_at {
            "published_at_ms"
        } else {
            "NULL"
        };
        let has_cloud: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('assets') WHERE name='cloud_state')",
                [],
                |r| r.get(0),
            )
            .map_err(super::db)?;
        let (cloud_column, gallery_released_column) = if has_cloud {
            ("cloud_state", "gallery_released")
        } else {
            ("'unknown'", "0")
        };
        let mut query = conn.prepare(&format!("SELECT rowid,id,manifest,received,processing,originals_released,{reason_column},{error_column},{received_at_column},{published_at_column},{cloud_column},{gallery_released_column} FROM assets WHERE {filter} AND (?3 IS NULL OR rowid<?3) ORDER BY rowid DESC LIMIT ?4 OFFSET ?6")).map_err(super::db)?;
        let records = query
            .query_map(
                params![
                    state,
                    kind,
                    before,
                    limit as i64 + i64::from(numbered_offset.is_none()),
                    sender,
                    numbered_offset.unwrap_or(0)
                ],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, bool>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<i64>>(8)?,
                        r.get::<_, Option<i64>>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, bool>(11)?,
                    ))
                },
            )
            .map_err(super::db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(super::db)?;
        let more = numbered_offset.is_none() && records.len() > limit;
        let mut items = Vec::new();
        let peers = super::devices::DeviceDirectory::open(&self.root, "en")?.peers()?;
        for (
            cursor,
            id,
            manifest,
            received,
            processing,
            originals_released,
            release_reason,
            processing_error,
            received_at_ms,
            published_at_ms,
            cloud_state,
            gallery_released,
        ) in records.into_iter().take(limit)
        {
            let asset: Asset = serde_json::from_str(&manifest)?;
            asset.validate()?;
            let total_bytes = asset.resources.iter().map(|r| r.size).sum();
            let confirmed_bytes = if received {
                total_bytes
            } else {
                asset
                    .resources
                    .iter()
                    .map(|r| {
                        let complete = self.root.join("blobs").join(&r.sha256);
                        let partial = self.root.join("partial").join(&r.sha256);
                        fs::metadata(complete)
                            .or_else(|_| fs::metadata(partial))
                            .map(|m| m.len().min(r.size))
                            .unwrap_or(0)
                    })
                    .sum()
            };
            let sender_ids = if has_senders {
                let mut q = conn
                    .prepare(
                        "SELECT sender_id FROM asset_senders WHERE asset_id=?1 ORDER BY sender_id",
                    )
                    .map_err(super::db)?;
                let ids = q
                    .query_map([&id], |r| r.get::<_, String>(0))
                    .map_err(super::db)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(super::db)?;
                ids
            } else {
                vec![]
            };
            let senders = peers
                .iter()
                .filter(|p| sender_ids.contains(&p.profile.id))
                .cloned()
                .collect();
            items.push(HistoryItem {
                cursor,
                id,
                filename: asset.resources[0].filename.clone(),
                kind: asset.kind,
                burst_primary: backupduck_core::BurstMetadata::from_fields(&asset.metadata)?
                    .map(|burst| burst.primary),
                total_bytes,
                confirmed_bytes,
                receipt: if received { "received" } else { "receiving" },
                processing,
                processing_error,
                captured_at_ms: asset
                    .metadata
                    .get("created_at_ms")
                    .and_then(|value| value.parse::<i64>().ok())
                    .filter(|value| *value > 0),
                received_at_ms,
                published_at_ms,
                originals_released,
                release_reason,
                cloud_state,
                gallery_released,
                senders,
            });
        }
        let next_cursor = if more {
            items.last().map(|i| i.cursor)
        } else {
            None
        };
        Ok(HistoryPage {
            items,
            total,
            next_cursor,
        })
    }
}
