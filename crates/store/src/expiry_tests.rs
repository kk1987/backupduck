use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "backupduck-expiry-{}-{}",
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
fn asset(name: &str, contents: &[&[u8]]) -> Asset {
    Asset {
        version: PROTOCOL_VERSION,
        source_id: name.into(),
        revision: "1".into(),
        kind: if contents.len() > 1 {
            AssetKind::Motion
        } else {
            AssetKind::Photo
        },
        metadata: Default::default(),
        resources: contents
            .iter()
            .enumerate()
            .map(|(i, bytes)| Resource {
                role: if i == 0 {
                    ResourceRole::Photo
                } else {
                    ResourceRole::PairedVideo
                },
                filename: if i == 0 { "photo.jpg" } else { "paired.mov" }.into(),
                media_type: if i == 0 {
                    "image/jpeg"
                } else {
                    "video/quicktime"
                }
                .into(),
                size: bytes.len() as u64,
                sha256: digest(bytes),
            })
            .collect(),
    }
}
fn reserved(r: &Receiver) -> u64 {
    r.overview().unwrap()["reserved_bytes"].as_u64().unwrap()
}
const TTL: u64 = ABANDONED_TTL_MS;
fn later() -> i64 {
    now_ms().unwrap() + TTL as i64 + 1
}

#[test]
fn expired_partial_upload_frees_its_reservation_and_file() {
    let t = Temp::new();
    let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
    let bytes = b"abandoned upload".as_slice();
    let id = r.register(asset("gone", &[bytes])).unwrap().asset_id;
    r.append(&id, &digest(bytes), 0, &bytes[..4], &digest(&bytes[..4]))
        .unwrap();
    r.attribute_sender(&id, &digest(b"sender")).unwrap();
    assert_eq!(reserved(&r), bytes.len() as u64);
    let partial = t.0.join("partial").join(digest(bytes));
    assert!(partial.exists());
    assert_eq!(
        r.expire_abandoned(later(), TTL).unwrap(),
        ExpiredSummary {
            assets: 1,
            bytes: bytes.len() as u64
        }
    );
    assert!(!partial.exists());
    assert_eq!(reserved(&r), 0);
    assert!(matches!(r.status(&id), Err(Error::NotFound)));
    let senders: i64 = r
        .conn
        .query_row("SELECT COUNT(*) FROM asset_senders", [], |row| row.get(0))
        .unwrap();
    assert_eq!(senders, 0);
    // Registering again starts a fresh reservation.
    r.register(asset("gone", &[bytes])).unwrap();
    assert_eq!(reserved(&r), bytes.len() as u64);
}

#[test]
fn content_shared_with_a_received_asset_survives() {
    let t = Temp::new();
    let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
    let shared = b"shared original".as_slice();
    let extra = b"never finished".as_slice();
    let kept = r.register(asset("kept", &[shared])).unwrap().asset_id;
    r.append(&kept, &digest(shared), 0, shared, &digest(shared))
        .unwrap();
    r.commit(&kept).unwrap();
    let id = r
        .register(asset("orphan", &[shared, extra]))
        .unwrap()
        .asset_id;
    assert_eq!(
        r.expire_abandoned(later(), TTL).unwrap(),
        ExpiredSummary {
            assets: 1,
            bytes: extra.len() as u64
        }
    );
    assert!(matches!(r.asset(&id), Err(Error::NotFound)));
    assert_eq!(r.status(&kept).unwrap().receipt, ReceiptState::Received);
    assert_eq!(
        fs::read(t.0.join("blobs").join(digest(shared))).unwrap(),
        shared
    );
    assert_eq!(reserved(&r), shared.len() as u64);
}

#[test]
fn recent_and_received_assets_are_untouched() {
    let t = Temp::new();
    let mut r = Receiver::open(&t.0, 1 << 20).unwrap();
    let bytes = b"still uploading".as_slice();
    let id = r.register(asset("recent", &[bytes])).unwrap().asset_id;
    let now = now_ms().unwrap();
    assert_eq!(
        r.expire_abandoned(now + TTL as i64 - 60_000, TTL).unwrap(),
        ExpiredSummary::default()
    );
    // An accepted chunk restarts the idle clock.
    r.conn
        .execute("UPDATE assets SET last_activity_ms=1 WHERE id=?1", [&id])
        .unwrap();
    r.append(&id, &digest(bytes), 0, &bytes[..3], &digest(&bytes[..3]))
        .unwrap();
    assert_eq!(
        r.expire_abandoned(now + 1000, TTL).unwrap(),
        ExpiredSummary::default()
    );
    r.append(&id, &digest(bytes), 3, &bytes[3..], &digest(&bytes[3..]))
        .unwrap();
    r.commit(&id).unwrap();
    // Received assets never expire, however old.
    assert_eq!(
        r.expire_abandoned(i64::MAX, 0).unwrap(),
        ExpiredSummary::default()
    );
    assert_eq!(r.status(&id).unwrap().receipt, ReceiptState::Received);
}

#[test]
fn database_without_activity_column_migrates_and_open_expires_stale_rows() {
    let t = Temp::new();
    let bytes = b"legacy partial".as_slice();
    let legacy = asset("legacy", &[bytes]);
    let id = legacy.id().unwrap();
    {
        let conn = Connection::open(t.0.join("receiver.sqlite3")).unwrap();
        conn.execute_batch(
            "CREATE TABLE assets(id TEXT PRIMARY KEY, manifest TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, processing TEXT NOT NULL DEFAULT 'not_requested', originals_released INTEGER NOT NULL DEFAULT 0, release_reason TEXT, processing_error TEXT, received_at_ms INTEGER, published_at_ms INTEGER);
             CREATE TABLE blobs(hash TEXT PRIMARY KEY, size INTEGER NOT NULL, ready INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets(id,manifest) VALUES(?1,?2)",
            params![id, serde_json::to_string(&legacy).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blobs(hash,size) VALUES(?1,?2)",
            params![digest(bytes), bytes.len() as i64],
        )
        .unwrap();
    }
    // An in-flight legacy upload gets a full TTL instead of expiring on upgrade.
    let r = Receiver::open(&t.0, 1 << 20).unwrap();
    assert_eq!(r.expired_at_open(), ExpiredSummary::default());
    assert_eq!(r.status(&id).unwrap().receipt, ReceiptState::Receiving);
    let page = catalog::Catalog::open(&t.0)
        .unwrap()
        .page(None, "all", "all", 10)
        .unwrap();
    assert_eq!(page.items.len(), 1);
    r.conn
        .execute("UPDATE assets SET last_activity_ms=1", [])
        .unwrap();
    drop(r);
    // Reopening must not add the column again, and expires the idle row.
    let r = Receiver::open(&t.0, 1 << 20).unwrap();
    assert_eq!(
        r.expired_at_open(),
        ExpiredSummary {
            assets: 1,
            bytes: bytes.len() as u64
        }
    );
    assert_eq!(reserved(&r), 0);
}
