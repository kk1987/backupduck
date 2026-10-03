//! Bootstrap: fetch the Photos page and read `WIZ_global_data`.

use crate::{Error, Result};
use reqwest::{cookie::Jar, redirect::Policy, StatusCode};
use serde_json::{Map, Value};
use std::{fmt, sync::Arc, time::Duration};

pub(crate) const ORIGIN: &str = "https://photos.google.com";

/// Request parameters read from the page's global data. The account email
/// (`oPEP7c`) is deliberately not retained.
#[derive(Clone, PartialEq, Eq)]
pub struct GlobalData {
    /// `SNlM0e`: XSRF token, sent as `at`.
    pub at: String,
    /// `FdrFJe`: sent as `f.sid`.
    pub f_sid: String,
    /// `cfb2h`: build label, sent as `bl`.
    pub bl: String,
    /// `Im6cmf` (gpwc), e.g. `/_/PhotosUi`; falls back to Toolkit's `eptZe`.
    pub rpc_path: String,
}

impl fmt::Debug for GlobalData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalData")
            .field("bl", &self.bl)
            .field("rpc_path", &self.rpc_path)
            .finish_non_exhaustive()
    }
}

/// Extracts `<script data-id="_gd">window.WIZ_global_data = {...};</script>`.
/// A page without it, or without the session keys, means the cookies are not
/// signed in.
pub fn parse_global_data(html: &str) -> Result<GlobalData> {
    let start = html.find("data-id=\"_gd\"").ok_or(Error::SessionExpired)?;
    let body = &html[start..];
    let body = &body[body.find('>').ok_or(Error::SessionExpired)? + 1..];
    let body = &body[..body.find("</script>").ok_or(Error::SessionExpired)?];
    let json = body
        .trim()
        .strip_prefix("window.WIZ_global_data")
        .and_then(|s| s.trim_start().strip_prefix('='))
        .ok_or(Error::SessionExpired)?
        .trim()
        .trim_end_matches(';');
    let data: Map<String, Value> = serde_json::from_str(json)
        .map_err(|_| Error::Parse("WIZ_global_data is not a JSON object".into()))?;
    let key = |k: &str| {
        data.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let rpc_path = key("Im6cmf")
        .or_else(|| key("eptZe").map(|p| p.trim_end_matches('/').to_owned()))
        .filter(|p| p.starts_with('/'))
        .ok_or(Error::SessionExpired)?;
    Ok(GlobalData {
        at: key("SNlM0e").ok_or(Error::SessionExpired)?,
        f_sid: key("FdrFJe").ok_or(Error::SessionExpired)?,
        bl: key("cfb2h").ok_or(Error::SessionExpired)?,
        rpc_path,
    })
}

pub fn base_path(account_index: u32) -> String {
    if account_index == 0 {
        "/".into()
    } else {
        format!("/u/{account_index}/")
    }
}

pub struct Session {
    pub(crate) client: reqwest::Client,
    pub(crate) base_path: String,
    pub(crate) globals: GlobalData,
    account_index: u32,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("account_index", &self.account_index)
            .field("globals", &self.globals)
            .finish_non_exhaustive()
    }
}

/// 401/403 and redirects (to the sign-in page) mean the cookies are stale.
pub(crate) fn check_status(status: StatusCode) -> Result<()> {
    match status.as_u16() {
        200..=299 => Ok(()),
        300..=399 | 401 | 403 => Err(Error::SessionExpired),
        429 => Err(Error::RateLimited),
        other => Err(Error::Http(other)),
    }
}

impl Session {
    pub async fn open(jar: Arc<Jar>, account_index: u32) -> Result<Self> {
        let client = reqwest::Client::builder()
            .cookie_provider(jar)
            .redirect(Policy::none())
            .user_agent(concat!(
                "backupduck-cloud-audit/",
                env!("CARGO_PKG_VERSION")
            ))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()?;
        let base_path = base_path(account_index);
        let response = client.get(format!("{ORIGIN}{base_path}")).send().await?;
        check_status(response.status())?;
        let globals = parse_global_data(&response.text().await?)?;
        Ok(Self {
            client,
            base_path,
            globals,
            account_index,
        })
    }

    pub fn account_index(&self) -> u32 {
        self.account_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_data() {
        let html = r#"<html><script nonce="x" data-id="_gd">window.WIZ_global_data = {"SNlM0e":"tok:1","FdrFJe":"-123","cfb2h":"boq_photosuiserver_1","Im6cmf":"/_/PhotosUi","oPEP7c":"someone@example.com"};</script></html>"#;
        let g = parse_global_data(html).unwrap();
        assert_eq!(g.at, "tok:1");
        assert_eq!(g.f_sid, "-123");
        assert_eq!(g.bl, "boq_photosuiserver_1");
        assert_eq!(g.rpc_path, "/_/PhotosUi");
        assert!(!format!("{g:?}").contains("example.com"));
        assert!(!format!("{g:?}").contains("tok:1"));

        let toolkit = html.replace(r#""Im6cmf":"/_/PhotosUi""#, r#""eptZe":"/_/PhotosUi/""#);
        assert_eq!(parse_global_data(&toolkit).unwrap().rpc_path, "/_/PhotosUi");

        let signed_out = html.replace(r#""SNlM0e":"tok:1","#, "");
        assert!(matches!(
            parse_global_data(&signed_out),
            Err(Error::SessionExpired)
        ));
        assert!(matches!(
            parse_global_data("<html></html>"),
            Err(Error::SessionExpired)
        ));
        assert_eq!(base_path(0), "/");
        assert_eq!(base_path(2), "/u/2/");
    }

    #[test]
    fn statuses() {
        assert!(check_status(StatusCode::OK).is_ok());
        assert!(matches!(
            check_status(StatusCode::FOUND),
            Err(Error::SessionExpired)
        ));
        assert!(matches!(
            check_status(StatusCode::FORBIDDEN),
            Err(Error::SessionExpired)
        ));
        assert!(matches!(
            check_status(StatusCode::TOO_MANY_REQUESTS),
            Err(Error::RateLimited)
        ));
        assert!(matches!(
            check_status(StatusCode::BAD_GATEWAY),
            Err(Error::Http(502))
        ));
    }
}
