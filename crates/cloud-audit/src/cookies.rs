//! Netscape `cookies.txt` loading. The file grants full Google account access:
//! values are never printed, and only google.com cookies are kept.

use crate::{Error, Result};
use reqwest::cookie::Jar;
use std::{fmt, path::Path, sync::Arc};

#[derive(Clone)]
struct Cookie {
    domain: String,
    include_subdomains: bool,
    path: String,
    secure: bool,
    name: String,
    value: String,
}

#[derive(Clone)]
pub struct CookieFile {
    cookies: Vec<Cookie>,
}

impl fmt::Debug for CookieFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CookieFile({} cookies)", self.cookies.len())
    }
}

fn is_google(domain: &str) -> bool {
    domain == "google.com" || domain.ends_with(".google.com")
}

impl CookieFile {
    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Parses the 7 tab-separated fields: domain, include-subdomains flag,
    /// path, secure flag, expiry, name, value. `#HttpOnly_` prefixes are
    /// stripped; other `#` lines and blank lines are comments.
    pub fn parse(text: &str) -> Result<Self> {
        let mut cookies = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.splitn(7, '\t').collect();
            if fields.len() != 7 {
                return Err(Error::Cookies(format!(
                    "line {} does not have 7 tab-separated fields",
                    index + 1
                )));
            }
            let domain = fields[0].to_ascii_lowercase();
            let host = domain.trim_start_matches('.');
            if !is_google(host) {
                continue;
            }
            cookies.push(Cookie {
                include_subdomains: fields[1].eq_ignore_ascii_case("TRUE")
                    || domain.starts_with('.'),
                domain: host.to_owned(),
                path: fields[2].to_owned(),
                secure: fields[3].eq_ignore_ascii_case("TRUE"),
                name: fields[5].to_owned(),
                value: fields[6].to_owned(),
            });
        }
        if cookies.is_empty() {
            return Err(Error::Cookies("no google.com cookies found".into()));
        }
        Ok(Self { cookies })
    }

    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// `(domain, name)` pairs, for diagnostics without values.
    pub fn names(&self) -> impl Iterator<Item = (&str, &str)> {
        self.cookies
            .iter()
            .map(|c| (c.domain.as_str(), c.name.as_str()))
    }

    /// Cookies are added as session cookies: the browser export's expiry is
    /// not enforced, Google rejects stale ones server-side.
    pub fn jar(&self) -> Arc<Jar> {
        let jar = Jar::default();
        for c in &self.cookies {
            let path = if c.path.starts_with('/') {
                &c.path
            } else {
                "/"
            };
            let Ok(url) = format!("https://{}{}", c.domain, path).parse() else {
                continue;
            };
            let mut header = format!("{}={}; Path={}", c.name, c.value, path);
            if c.include_subdomains {
                header.push_str("; Domain=");
                header.push_str(&c.domain);
            }
            if c.secure {
                header.push_str("; Secure");
            }
            jar.add_cookie_str(&header, &url);
        }
        Arc::new(jar)
    }
}

/// Returns `true` when the file is private to its owner, `false` when group
/// or others can read it.
#[cfg(unix)]
pub fn check_permissions(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o044 == 0)
}
