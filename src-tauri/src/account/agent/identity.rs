//! This machine as the website knows it: its install id (the merge key that keeps one machine
//! one agent), its name, CPU, OS, the app's version, and what it can run.

use crate::error::Result;
use crate::platform::AppDirs;

/// A stable identifier for this installation, generated once and kept in the app data
/// dir. It is the merge key for the agent, so reinstalling re-registers the same machine
/// and two machines never collide even when they share a display name.
pub(super) fn install_id(dirs: &AppDirs) -> Result<String> {
    install_id_at(&dirs.data()?.join("install-id"))
}

/// The install id kept at `path`, generated and written there the first time.
fn install_id_at(path: &std::path::Path) -> Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let id = random_hex();
    crate::platform::atomic_file::write(path, &id)?;
    Ok(id)
}

/// 16 random bytes as hex: install ids, co-location tokens and nonces.
pub(super) fn random_hex() -> String {
    crate::platform::random_hex(16)
}

/// A human label for this machine (editable server-side later). Not an identifier.
pub(super) fn machine_name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).ok();
    machine_name_from(std::env::var("CYBERCTF_AGENT_NAME").ok(), user, std::env::consts::OS)
}

/// The machine label from an explicit name, else the user and the OS.
fn machine_name_from(explicit: Option<String>, user: Option<String>, os: &str) -> String {
    if let Some(n) = explicit
        && !n.trim().is_empty()
    {
        return n.trim().to_string();
    }
    let user = user.filter(|s| !s.trim().is_empty());
    let os = match os {
        "macos" => "Mac",
        "windows" => "PC",
        "linux" => "Linux",
        other => other,
    };
    match user {
        Some(u) => format!("{u}'s {os}"),
        None => format!("Cyber CTF {os}"),
    }
}

pub(super) fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// The launcher app's own version (semver), reported so the website can tell which build each
/// machine runs. The crate version is the app's, bumped with `tauri.conf.json` and `package.json`
/// at each release (as the About menu shows it).
pub(super) fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// This machine's OS as the website shows it: `"macos 15.1"`, `"windows 11"`, `"linux 24.04"`, or
/// the family alone when the version can't be read.
pub(super) fn os_label() -> String {
    os_label_from(std::env::consts::OS, sysinfo::System::os_version().as_deref())
}

/// The OS label from its family and the version the OS reports, lowercased; only the version's
/// first word is kept (Windows reports `"11 (22631)"`, the build in brackets).
fn os_label_from(family: &str, version: Option<&str>) -> String {
    let family = family.trim().to_lowercase();
    match version.and_then(|v| v.split_whitespace().next()) {
        Some(v) => format!("{family} {}", v.to_lowercase()),
        None => family,
    }
}

/// What this machine can run. Docker for now; VM providers are detected later.
pub(super) fn capabilities() -> Vec<String> {
    vec!["docker".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_id_is_made_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("cyberctf-install-{}", random_hex()));
        let path = dir.join("data").join("install-id");
        let first = install_id_at(&path).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(install_id_at(&path).unwrap(), first);
        std::fs::write(&path, "  kept-id \n").unwrap();
        assert_eq!(install_id_at(&path).unwrap(), "kept-id");
        std::fs::write(&path, "   ").unwrap();
        let regenerated = install_id_at(&path).unwrap();
        assert_eq!(regenerated.len(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_machine_name_prefers_the_explicit_name_then_the_user() {
        assert_eq!(machine_name_from(Some("  Lab box ".into()), Some("ada".into()), "linux"), "Lab box");
        assert_eq!(machine_name_from(Some("  ".into()), Some("ada".into()), "macos"), "ada's Mac");
        assert_eq!(machine_name_from(None, Some("bob".into()), "windows"), "bob's PC");
        assert_eq!(machine_name_from(None, Some(" ".into()), "linux"), "Cyber CTF Linux");
        assert_eq!(machine_name_from(None, None, "freebsd"), "Cyber CTF freebsd");
        assert_ne!(machine_name(), "");
        assert_eq!(capabilities(), vec!["docker".to_string()]);
        assert_eq!(arch(), std::env::consts::ARCH);
    }

    #[test]
    fn this_machine_reports_the_app_version_and_its_os_family_and_version() {
        assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(os_label_from("macos", Some("15.1")), "macos 15.1");
        assert_eq!(os_label_from("windows", Some("11 (22631)")), "windows 11", "the build is dropped");
        assert_eq!(os_label_from("Linux", Some("24.04")), "linux 24.04", "lowercased");
        assert_eq!(os_label_from("linux", None), "linux", "the family alone without a version");
        assert_eq!(os_label_from("linux", Some("  ")), "linux", "a blank version is none");
        assert!(os_label().starts_with(std::env::consts::OS));
    }
}
