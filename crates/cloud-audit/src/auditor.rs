//! Throttled lookups on top of [`Session`].

use crate::{
    parser::{self, HashLookup, ItemInfo, LockedMove},
    sha1_hex_to_base64, Error, Result, Session,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Keys or hashes per RPC.
pub const CHUNK: usize = 50;

/// Call pacing. Calls are sequential (`&mut self`), so at most one is in flight.
#[derive(Clone, Copy, Debug)]
pub struct Throttle {
    /// Minimum spacing between call starts.
    pub min_interval: Duration,
    /// Wait after HTTP 429 before the single retry.
    pub rate_limit_backoff: Duration,
    /// RPC calls allowed per run, including 429 retries.
    pub max_calls: u32,
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_millis(300),
            rate_limit_backoff: Duration::from_secs(30),
            max_calls: 24,
        }
    }
}

pub struct Auditor {
    session: Session,
    throttle: Throttle,
    calls: u32,
    last_call: Option<Instant>,
}

impl Auditor {
    pub fn new(session: Session, throttle: Throttle) -> Self {
        Self {
            session,
            throttle,
            calls: 0,
            last_call: None,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn calls(&self) -> u32 {
        self.calls
    }

    async fn paced(&mut self, rpcid: &str, payload: &Value) -> Result<Value> {
        if self.calls >= self.throttle.max_calls {
            return Err(Error::BudgetExhausted);
        }
        if let Some(last) = self.last_call {
            let ready = last + self.throttle.min_interval;
            tokio::time::sleep_until(ready.into()).await;
        }
        self.calls += 1;
        self.last_call = Some(Instant::now());
        self.session.batchexecute(rpcid, payload).await
    }

    async fn call(&mut self, rpcid: &str, payload: &Value) -> Result<Value> {
        match self.paced(rpcid, payload).await {
            Err(Error::RateLimited) => {
                tokio::time::sleep(self.throttle.rate_limit_backoff).await;
                self.paced(rpcid, payload).await
            }
            other => other,
        }
    }

    /// Looks up hex SHA-1 digests with `swbisb`. The result is aligned with
    /// the input; `None` means no library item has that hash.
    pub async fn lookup_hashes(&mut self, sha1_hex: &[String]) -> Result<Vec<Option<HashLookup>>> {
        let hashes = sha1_hex
            .iter()
            .map(|h| sha1_hex_to_base64(h))
            .collect::<Result<Vec<_>>>()?;
        let mut found = HashMap::new();
        for chunk in hashes.chunks(CHUNK) {
            let response = self.call("swbisb", &json!([chunk, null, 3, 0])).await?;
            for m in parser::parse_hash_matches(&response)? {
                found.insert(m.hash_b64.clone(), m);
            }
        }
        Ok(hashes.iter().map(|h| found.get(h).cloned()).collect())
    }

    /// Batch item info with `EWgK9e`, aligned with the input media keys.
    pub async fn item_info(&mut self, media_keys: &[String]) -> Result<Vec<Option<ItemInfo>>> {
        let mut found = HashMap::new();
        for chunk in media_keys.chunks(CHUNK) {
            let response = self.call("EWgK9e", &batch_info_payload(chunk)).await?;
            for info in parser::parse_batch_info(&response)? {
                found.insert(info.media_key.clone(), info);
            }
        }
        Ok(media_keys.iter().map(|k| found.get(k).cloned()).collect())
    }

    /// Single item info with `VrseUb`; lower confidence than [`Self::item_info`].
    pub async fn item_info_single(&mut self, media_key: &str) -> Result<ItemInfo> {
        let response = self
            .call("VrseUb", &json!([media_key, null, null, null, null]))
            .await?;
        parser::parse_item_info(&response)
    }

    /// Moves library items, by dedup key, into the Locked Folder with
    /// `StLnCe`. Works from a plain cookie session (verified live 2026-10-03).
    /// The caller must confirm the move with a fresh hash lookup.
    pub async fn move_to_locked_folder(&mut self, dedup_keys: &[String]) -> Result<LockedMove> {
        let mut moved = LockedMove::default();
        for chunk in dedup_keys.chunks(CHUNK) {
            let response = self.call("StLnCe", &locked_move_payload(chunk)).await?;
            let part = parser::parse_locked_move(&response)?;
            moved.new_keys.extend(part.new_keys);
            moved.removed_keys.extend(part.removed_keys);
        }
        Ok(moved)
    }
}

/// `[[dedup_key, ..], []]`.
pub fn locked_move_payload(dedup_keys: &[String]) -> Value {
    json!([dedup_keys, []])
}

/// `[[[ [[k1],[k2],..] ], [[null x24, [], null x10, []]]]]`, as Toolkit
/// `getBatchMediaInfo` and gpwc `GetBatchMediaInfo` send it.
pub fn batch_info_payload(media_keys: &[String]) -> Value {
    let keys: Vec<Value> = media_keys.iter().map(|k| json!([k])).collect();
    let mut fields = vec![Value::Null; 24];
    fields.push(json!([]));
    fields.extend(std::iter::repeat_n(Value::Null, 10));
    fields.push(json!([]));
    json!([[[keys], [fields]]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_payload_shape() {
        let payload = batch_info_payload(&["a".into(), "b".into()]);
        assert_eq!(payload[0][0], json!([[["a"], ["b"]]]));
        let fields = payload[0][1][0].as_array().unwrap();
        assert_eq!(fields.len(), 36);
        assert_eq!(fields[24], json!([]));
        assert_eq!(fields[35], json!([]));
        assert!(fields[..24].iter().all(Value::is_null));
        assert!(fields[25..35].iter().all(Value::is_null));
    }

    #[test]
    fn locked_move_payload_shape() {
        assert_eq!(
            locked_move_payload(&["d1".into(), "d2".into()]),
            json!([["d1", "d2"], []])
        );
    }
}
