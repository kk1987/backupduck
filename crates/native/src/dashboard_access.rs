//! Private browser credentials, separate from transfer pairing and diagnostics.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    hash: String,
    expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Access {
    code: String,
    sessions: Vec<Session>,
    failures: u8,
    locked_until: u64,
    #[serde(skip)]
    path: PathBuf,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn valid_code(code: &str) -> bool {
    code.len() == 10 && code.bytes().all(|b| b.is_ascii_digit())
}
impl Access {
    pub(super) fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let path = root.join("dashboard-access.json");
        if path.exists() {
            let mut access: Self = serde_json::from_slice(&fs::read(&path)?)?;
            if !valid_code(&access.code)
                || access.sessions.len() > 64
                || access.sessions.iter().any(|s| s.hash.len() != 64)
            {
                return Err(Error::Integrity);
            }
            access.path = path;
            return Ok(access);
        }
        let access = Self {
            code: dashboard::random_code()?,
            sessions: vec![],
            failures: 0,
            locked_until: 0,
            path,
        };
        access.save()?;
        Ok(access)
    }
    fn save(&self) -> Result<()> {
        let temp = self
            .path
            .with_extension(format!("{}.tmp", dashboard::random_hex()?));
        private_write(&temp, &serde_json::to_vec(self)?)?;
        let result = fs::rename(&temp, &self.path);
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.map_err(Error::from)
    }
    pub(super) fn code(&self) -> &str {
        &self.code
    }
    pub(super) fn change(&mut self, code: Option<String>, reset: bool, revoke: bool) -> Result<()> {
        if code.is_some() && reset {
            return Err(Error::Invalid("dashboard code choice".into()));
        }
        let changing_code = code.is_some();
        let mut next = self.clone();
        if let Some(code) = code {
            if !valid_code(&code) {
                return Err(Error::Invalid("dashboard code".into()));
            }
            next.code = code;
        }
        if reset {
            next.code = dashboard::random_code()?;
        }
        if revoke || reset || changing_code {
            next.sessions.clear();
            next.failures = 0;
            next.locked_until = 0;
        }
        next.save()?;
        *self = next;
        Ok(())
    }
    pub(super) fn login(&mut self, code: &str, remember: bool) -> Result<String> {
        let time = now();
        if self.locked_until > time {
            return Err(Error::Conflict("dashboard locked".into()));
        }
        let mut next = self.clone();
        if !dashboard::equal_secret(code, &next.code) {
            next.failures += 1;
            if next.failures >= 5 {
                next.locked_until = time + 300;
                next.failures = 0;
            }
            next.save()?;
            *self = next;
            return Err(Error::Unauthorized);
        }
        let token = dashboard::random_hex()?;
        next.failures = 0;
        next.locked_until = 0;
        next.sessions.retain(|s| s.expires > time);
        if next.sessions.len() >= 64 {
            next.sessions.remove(0);
        }
        next.sessions.push(Session {
            hash: backupduck_core::digest(token.as_bytes()),
            expires: time + if remember { 30 * 86400 } else { 12 * 3600 },
        });
        next.save()?;
        *self = next;
        Ok(token)
    }
    pub(super) fn authorized(&self, token: &str) -> bool {
        let hash = backupduck_core::digest(token.as_bytes());
        self.sessions
            .iter()
            .any(|s| s.expires > now() && dashboard::equal_secret(&s.hash, &hash))
    }
    pub(super) fn logout(&mut self, token: &str) -> Result<()> {
        let hash = backupduck_core::digest(token.as_bytes());
        let mut next = self.clone();
        next.sessions
            .retain(|s| !dashboard::equal_secret(&s.hash, &hash));
        next.save()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_and_sessions_survive_restart_and_changes_revoke_every_browser() {
        let root = std::env::temp_dir().join(format!(
            "backupduck-access-{}",
            dashboard::random_hex().unwrap()
        ));
        let mut a = Access::open(&root).unwrap();
        a.change(Some("0123456789".into()), false, false).unwrap();
        let token = a.login("0123456789", true).unwrap();
        let other = a.login("0123456789", false).unwrap();
        assert_ne!(token, other);
        assert!(!String::from_utf8(fs::read(&a.path).unwrap())
            .unwrap()
            .contains(&token));
        let mut a = Access::open(&root).unwrap();
        assert_eq!(a.code(), "0123456789");
        assert!(a.authorized(&token) && a.authorized(&other));
        a.logout(&token).unwrap();
        assert!(!a.authorized(&token) && a.authorized(&other));
        a.change(None, false, true).unwrap();
        assert!(!a.authorized(&other));
        let token = a.login("0123456789", true).unwrap();
        a.change(Some("9876543210".into()), false, false).unwrap();
        assert!(!a.authorized(&token));
        assert!(a.login("0123456789", true).is_err());
        assert!(a.change(Some("bad".into()), false, false).is_err());
        assert_eq!(a.code(), "9876543210");
        a.change(None, true, false).unwrap();
        assert!(valid_code(a.code()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn expired_sessions_and_restart_do_not_bypass_lockout() {
        let root = std::env::temp_dir().join(format!(
            "backupduck-access-{}",
            dashboard::random_hex().unwrap()
        ));
        let mut a = Access::open(&root).unwrap();
        a.change(Some("0123456789".into()), false, false).unwrap();
        let token = a.login("0123456789", true).unwrap();
        a.sessions[0].expires = 0;
        assert!(!a.authorized(&token));
        for _ in 0..5 {
            assert!(matches!(a.login("wrong", false), Err(Error::Unauthorized)));
        }
        let mut a = Access::open(&root).unwrap();
        assert!(matches!(
            a.login("0123456789", false),
            Err(Error::Conflict(_))
        ));
        a.locked_until = 0;
        assert!(a.login("0123456789", false).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
