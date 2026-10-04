//! Client for the undocumented Google Photos web RPC (`batchexecute`), used to
//! check whether uploaded originals exist in a library and whether they count
//! against storage quota. Protocol details follow xob0t/Google-Photos-Toolkit
//! and xob0t/google_photos_web_client (both MIT); see README.md.

pub mod auditor;
pub mod cookies;
pub mod parser;
pub mod rpc;
pub mod session;

pub use auditor::{Auditor, Throttle};
pub use cookies::CookieFile;
pub use parser::{HashLookup, ItemInfo, LockedMove};
pub use session::Session;

use base64::Engine;
use serde::Serialize;
use sha1::{Digest, Sha1};
use std::io::Read;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Google Photos session expired, or the account index is not signed in")]
    SessionExpired,
    #[error("rate limited by Google Photos")]
    RateLimited,
    #[error("unexpected HTTP status {0}")]
    Http(u16),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("unexpected response: {0}")]
    Parse(String),
    #[error("per-run RPC call budget exhausted")]
    BudgetExhausted,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cookies: {0}")]
    Cookies(String),
}
pub type Result<T> = std::result::Result<T, Error>;

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        match error.status() {
            Some(status) => Self::Http(status.as_u16()),
            // Strip the URL: it carries session parameters.
            None => Self::Transport(error.without_url().to_string()),
        }
    }
}

/// Converts a lowercase or uppercase hex SHA-1 into the base64 form that
/// `swbisb` expects.
pub fn sha1_hex_to_base64(hex: &str) -> Result<String> {
    if hex.len() != 40 {
        return Err(Error::Parse("SHA-1 hex must be 40 characters".into()));
    }
    let mut bytes = [0u8; 20];
    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
        let digit = |c: u8| (c as char).to_digit(16);
        match (digit(pair[0]), digit(pair[1])) {
            (Some(hi), Some(lo)) => bytes[i] = (hi * 16 + lo) as u8,
            _ => return Err(Error::Parse("SHA-1 hex contains a non-hex digit".into())),
        }
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Lowercase hex SHA-1 of a reader's bytes.
pub fn sha1_hex(mut reader: impl Read) -> Result<String> {
    let mut hasher = Sha1::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// In the library and reported as not taking up storage.
    Free,
    /// In the library and reported as taking up storage.
    CountsAgainstQuota,
    /// No library item has this file's SHA-1.
    NotFound,
    /// Found, but the quota flag is missing or the item info was not returned.
    Unknown,
}

pub fn verdict(lookup: Option<&HashLookup>, info: Option<&ItemInfo>) -> Verdict {
    match (lookup, info.and_then(|i| i.takes_up_space)) {
        (None, _) => Verdict::NotFound,
        (Some(_), Some(false)) => Verdict::Free,
        (Some(_), Some(true)) => Verdict::CountsAgainstQuota,
        (Some(_), None) => Verdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_to_base64() {
        // SHA-1 of the empty string.
        let hex = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        assert_eq!(
            sha1_hex_to_base64(hex).unwrap(),
            "2jmj7l5rSw0yVb/vlWAYkK/YBwk="
        );
        assert_eq!(
            sha1_hex_to_base64(&hex.to_uppercase()).unwrap(),
            "2jmj7l5rSw0yVb/vlWAYkK/YBwk="
        );
        assert_eq!(sha1_hex(&b""[..]).unwrap(), hex);
        assert!(sha1_hex_to_base64("da39").is_err());
        assert!(sha1_hex_to_base64(&"zz".repeat(20)).is_err());
        assert!(sha1_hex_to_base64(&"é".repeat(20)).is_err());
    }

    #[test]
    fn verdicts() {
        let lookup = HashLookup {
            hash_b64: "h".into(),
            media_key: "m".into(),
            dedup_key: None,
            device_model: None,
            width: None,
            height: None,
            timestamp_ms: None,
            creation_timestamp_ms: None,
        };
        let info = |takes_up_space| ItemInfo {
            media_key: "m".into(),
            file_name: None,
            size: None,
            takes_up_space,
            space_taken: None,
            is_original_quality: None,
        };
        assert_eq!(verdict(None, None), Verdict::NotFound);
        assert_eq!(verdict(None, Some(&info(Some(true)))), Verdict::NotFound);
        assert_eq!(verdict(Some(&lookup), None), Verdict::Unknown);
        assert_eq!(verdict(Some(&lookup), Some(&info(None))), Verdict::Unknown);
        assert_eq!(
            verdict(Some(&lookup), Some(&info(Some(false)))),
            Verdict::Free
        );
        assert_eq!(
            verdict(Some(&lookup), Some(&info(Some(true)))),
            Verdict::CountsAgainstQuota
        );
    }
}
