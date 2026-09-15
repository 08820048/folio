//! Windows auto-update. Packaged installs (Inno, with `unins000.exe` beside
//! Folio.exe) read `windows-appcast.json` from R2, the same BUILD number
//! Sparkle uses on macOS. `cargo run` has no uninstaller and does not check.

use serde::Deserialize;
#[cfg(windows)]
use std::io;

pub const FEED_PATH: &str = "windows-appcast.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Feed {
    pub version: String,
    pub build: u64,
    pub url: String,
    pub sha256: String,
}

pub fn current_build() -> u64 {
    env!("FOLIO_BUILD")
        .parse()
        .expect("FOLIO_BUILD is a number")
}

pub fn current_version() -> &'static str {
    env!("FOLIO_VERSION")
}

pub fn feed_url() -> String {
    format!("{}/{}", env!("FOLIO_PUBLIC_BASE"), FEED_PATH)
}

pub fn public_base() -> &'static str {
    env!("FOLIO_PUBLIC_BASE")
}

/// True when this process is the Inno-installed copy, not `cargo run`.
pub fn installed() -> bool {
    #[cfg(windows)]
    {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let Some(dir) = exe.parent() else {
            return false;
        };
        dir.join("unins000.exe").is_file()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn is_newer(feed: &Feed, running_build: u64) -> bool {
    feed.build > running_build
}

pub fn parse_feed(json: &str) -> Result<Feed, String> {
    let feed: Feed = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if feed.version.is_empty() || feed.build == 0 {
        return Err("Update feed is missing a version".into());
    }
    if !valid_sha256(&feed.sha256) {
        return Err("Update feed has a bad checksum".into());
    }
    if !allowed_url(&feed.url) {
        return Err("Update feed points outside Folio's download host".into());
    }
    Ok(feed)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn allowed_url(url: &str) -> bool {
    let Some(name) = url
        .strip_prefix(public_base())
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    url.starts_with("https://")
        && name.ends_with(".exe")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

/// Latest published installer, if the feed is newer than `running_build`.
#[cfg(windows)]
pub fn fetch_feed() -> io::Result<Feed> {
    let json = http_get(&feed_url())?;
    parse_feed(&json).map_err(io::Error::other)
}

#[cfg(windows)]
pub fn start_installer(path: &std::path::Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(path)
        .args(["/SILENT", "/NORESTART"])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(windows)]
fn http_get(url: &str) -> io::Result<String> {
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(io::Error::other)?;
    response.into_string().map_err(io::Error::other)
}

#[cfg(windows)]
pub fn download_installer(feed: &Feed) -> io::Result<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    const LIMIT: u64 = 400 * 1024 * 1024;
    let dir = std::env::temp_dir();
    let path = dir.join(format!("Folio-update-{}.exe", feed.build));
    let response = ureq::get(&feed.url)
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(io::Error::other)?;
    if let Some(len) = response.header("Content-Length")
        && let Ok(n) = len.parse::<u64>()
        && n > LIMIT
    {
        return Err(io::Error::other("Update is larger than expected"));
    }
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > LIMIT {
            return Err(io::Error::other("Update is larger than expected"));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
    file.flush()?;
    let digest = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !digest.eq_ignore_ascii_case(&feed.sha256) {
        let _ = std::fs::remove_file(&path);
        return Err(io::Error::other("Update checksum did not match"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(url: &str, sha: &str) -> String {
        format!(r#"{{"version":"1.2.3","build":99,"url":"{url}","sha256":"{sha}"}}"#)
    }

    fn ok_sha() -> &'static str {
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }

    #[test]
    fn parse_feed_accepts_a_file_on_the_public_host() {
        let url = format!("{}/Folio-1.2.3-99-windows-x64.exe", public_base());
        let feed = parse_feed(&sample(&url, ok_sha())).unwrap();
        assert_eq!(feed.version, "1.2.3");
        assert_eq!(feed.build, 99);
        assert!(is_newer(&feed, 5));
        assert!(!is_newer(&feed, 99));
        assert!(!is_newer(&feed, 100));
    }

    #[test]
    fn parse_feed_rejects_a_foreign_url() {
        assert!(parse_feed(&sample("https://evil.example/Folio.exe", ok_sha())).is_err());
        assert!(parse_feed(&sample("http://example/Folio.exe", ok_sha())).is_err());
        let sneak = format!("{}/../other.exe", public_base());
        assert!(parse_feed(&sample(&sneak, ok_sha())).is_err());
    }

    #[test]
    fn parse_feed_rejects_a_short_checksum() {
        let url = format!("{}/Folio.exe", public_base());
        assert!(parse_feed(&sample(&url, "abc")).is_err());
    }
}
