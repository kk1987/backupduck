//! Cloud-backup verification wire types. A receiver records verdicts reported
//! by an external auditor; it never contacts a cloud service itself.
use crate::*;

/// Most items accepted by one listing page or observation request.
pub const MAX_CLOUD_ITEMS: usize = 100;
/// Largest accepted observation request body.
pub const MAX_CLOUD_OBSERVATION_BYTES: usize = 64 * 1024;

/// Lowercase hex SHA-1 of gallery-copy bytes, as used by cloud media lookups.
pub fn valid_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudDueItem {
    pub asset_id: String,
    pub sha1: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub display_name: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub published_at_ms: Option<i64>,
    pub cloud_state: String,
    pub cloud_checks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudDuePage {
    pub items: Vec<CloudDueItem>,
    pub next: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudResult {
    Free,
    CountsAgainstQuota,
    NotFound,
}

/// A copy still not found a week after publication is reported missing.
pub const CLOUD_MISSING_AFTER_MS: i64 = 7 * 86_400_000;

/// State after applying one verdict. `verified` never regresses; quota and
/// missing copies can still become `verified`.
pub fn next_cloud_state(
    current: &str,
    result: CloudResult,
    published_at_ms: Option<i64>,
    now_ms: i64,
) -> &str {
    match (current, result) {
        (_, CloudResult::Free) | ("verified", _) => "verified",
        (_, CloudResult::CountsAgainstQuota) => "verified_counts_against_quota",
        (current, CloudResult::NotFound) => {
            if published_at_ms.is_some_and(|p| now_ms.saturating_sub(p) > CLOUD_MISSING_AFTER_MS) {
                "missing"
            } else if current == "unknown" {
                "pending"
            } else {
                current
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudObservation {
    pub asset_id: String,
    pub sha1: String,
    pub result: CloudResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
}
impl CloudObservation {
    pub fn validate(&self) -> Result<()> {
        if !valid_digest(&self.asset_id)
            || !valid_sha1(&self.sha1)
            || self.media_key.as_deref().is_some_and(|v| !valid_label(v))
            || self
                .device_model
                .as_deref()
                .is_some_and(|v| !valid_label(v))
        {
            return Err(Error::Invalid("cloud observation".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudObservations {
    pub observations: Vec<CloudObservation>,
}
impl CloudObservations {
    pub fn validate(&self) -> Result<()> {
        if self.observations.len() > MAX_CLOUD_ITEMS {
            return Err(Error::Invalid("too many cloud observations".into()));
        }
        self.observations
            .iter()
            .try_for_each(CloudObservation::validate)
    }
}

/// Resulting state counts for one observation request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSummary {
    pub verified: u32,
    pub quota: u32,
    pub still_pending: u32,
    pub missing: u32,
    pub rejected: u32,
}
