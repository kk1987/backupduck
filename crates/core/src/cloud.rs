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
    /// The cloud item predates this copy's publication: another device
    /// uploaded the same bytes first. Only for `cloud_preexisting` receivers.
    AlreadyInCloud,
    NotFound,
}

/// A copy still not found a week after publication is reported missing.
pub const CLOUD_MISSING_AFTER_MS: i64 = 7 * 86_400_000;

/// Cloud states whose bytes are known to be in the cloud library.
pub const CLOUD_HELD_STATES: [&str; 2] = ["verified", "verified_elsewhere"];

/// State after applying one verdict. `verified` never regresses and
/// `verified_elsewhere` only upgrades to `verified`; quota and missing copies
/// can still become either.
pub fn next_cloud_state(
    current: &str,
    result: CloudResult,
    published_at_ms: Option<i64>,
    now_ms: i64,
) -> &str {
    match (current, result) {
        (_, CloudResult::Free) | ("verified", _) => "verified",
        (_, CloudResult::AlreadyInCloud) | ("verified_elsewhere", _) => "verified_elsewhere",
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
    /// EXIF camera model reported by Google Photos, informational only; it is
    /// not the device that uploaded the file.
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
    #[serde(default)]
    pub already_in_cloud: u32,
    pub quota: u32,
    pub still_pending: u32,
    pub missing: u32,
    pub rejected: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 30 * 86_400_000;

    fn next(current: &str, result: CloudResult) -> &str {
        next_cloud_state(current, result, Some(NOW - 3_600_000), NOW)
    }

    #[test]
    fn transitions() {
        use CloudResult::*;
        assert_eq!(next("pending", AlreadyInCloud), "verified_elsewhere");
        assert_eq!(next("unknown", AlreadyInCloud), "verified_elsewhere");
        assert_eq!(next("missing", AlreadyInCloud), "verified_elsewhere");
        // Rows recorded as quota before the preexisting check re-classify.
        assert_eq!(
            next("verified_counts_against_quota", AlreadyInCloud),
            "verified_elsewhere"
        );
        assert_eq!(next("verified_elsewhere", Free), "verified");
        assert_eq!(
            next("verified_elsewhere", CountsAgainstQuota),
            "verified_elsewhere"
        );
        assert_eq!(next("verified_elsewhere", NotFound), "verified_elsewhere");
        assert_eq!(next("verified", AlreadyInCloud), "verified");
        assert_eq!(next("verified", CountsAgainstQuota), "verified");
        assert_eq!(
            next("pending", CountsAgainstQuota),
            "verified_counts_against_quota"
        );
        assert_eq!(next("verified_counts_against_quota", Free), "verified");
        assert_eq!(next("unknown", NotFound), "pending");
        assert_eq!(
            next_cloud_state("pending", NotFound, Some(0), NOW),
            "missing"
        );
    }

    #[test]
    fn result_wire_names() {
        assert_eq!(
            serde_json::to_value(CloudResult::AlreadyInCloud).unwrap(),
            "already_in_cloud"
        );
        assert_eq!(
            serde_json::from_str::<CloudResult>("\"already_in_cloud\"").unwrap(),
            CloudResult::AlreadyInCloud
        );
    }
}
