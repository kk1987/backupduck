use backupduck_cloud_audit::{CookieFile, Error};
use reqwest::cookie::CookieStore;

const FILE: &str = "# Netscape HTTP Cookie File\n\
# https://curl.se/docs/http-cookies.html\n\
\n\
.google.com\tTRUE\t/\tTRUE\t1893456000\tSID\tsid-value\n\
#HttpOnly_.google.com\tTRUE\t/\tTRUE\t1893456000\t__Secure-1PSID\tsecure-value\n\
photos.google.com\tFALSE\t/\tTRUE\t1893456000\tOSID\tosid-value\n\
#HttpOnly_accounts.google.com\tFALSE\t/\tTRUE\t1893456000\t__Host-GAPS\tgaps-value\n\
.youtube.com\tTRUE\t/\tTRUE\t1893456000\tYSC\tyt-value\n\
evilgoogle.com\tFALSE\t/\tFALSE\t0\tX\tevil\n\
.google.com.evil.example\tTRUE\t/\tFALSE\t0\tY\tevil\n";

#[test]
fn parses_and_filters() {
    let file = CookieFile::parse(FILE).unwrap();
    let names: Vec<_> = file.names().collect();
    assert_eq!(
        names,
        [
            ("google.com", "SID"),
            ("google.com", "__Secure-1PSID"),
            ("photos.google.com", "OSID"),
            ("accounts.google.com", "__Host-GAPS"),
        ]
    );
    let debug = format!("{file:?}");
    assert_eq!(debug, "CookieFile(4 cookies)");
    assert!(!debug.contains("value"));
}

#[test]
fn jar_sends_google_cookies() {
    let jar = CookieFile::parse(FILE).unwrap().jar();
    let header = jar
        .cookies(&"https://photos.google.com/".parse().unwrap())
        .unwrap();
    let header = header.to_str().unwrap();
    assert!(header.contains("SID=sid-value"));
    assert!(header.contains("__Secure-1PSID=secure-value"));
    assert!(header.contains("OSID=osid-value"));
    assert!(!header.contains("gaps-value"));
    assert!(!header.contains("evil"));
}

#[test]
fn rejects_bad_files() {
    assert!(matches!(
        CookieFile::parse("# only comments\n"),
        Err(Error::Cookies(_))
    ));
    assert!(matches!(
        CookieFile::parse(".youtube.com\tTRUE\t/\tTRUE\t0\tA\tb\n"),
        Err(Error::Cookies(_))
    ));
    let Err(Error::Cookies(message)) = CookieFile::parse(".google.com\tTRUE\t/\tsecret\n") else {
        panic!("short line accepted");
    };
    assert!(message.contains("line 1"));
    assert!(!message.contains("secret"));
}

#[cfg(unix)]
#[test]
fn permission_check() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("cloud-audit-cookies-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cookies.txt");
    std::fs::write(&path, FILE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(backupduck_cloud_audit::cookies::check_permissions(&path));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!backupduck_cloud_audit::cookies::check_permissions(&path));
    assert_eq!(CookieFile::load(&path).unwrap().len(), 4);
    std::fs::remove_dir_all(&dir).unwrap();
}
