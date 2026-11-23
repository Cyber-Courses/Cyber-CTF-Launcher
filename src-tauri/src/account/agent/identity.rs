//! This machine as the website knows it: its install id (the merge key that keeps one machine
//! one agent), its name, CPU, OS, the app's version, and what it can run.

use crate::error::Result;
use crate::platform::AppDirs;
use isothrum_machine::HostOs;

/// A stable identifier for this installation, generated once and kept in the launcher's store
/// (1.9 kept it in the `install-id` file, which the store takes in). It is the merge key for the
/// agent, so reinstalling re-registers the same machine and two machines never collide even when
/// they share a display name.
pub(super) fn install_id(dirs: &AppDirs) -> Result<String> {
    install_id_in(&crate::store::open(dirs)?)
}

/// The install id `store` keeps, made and kept there the first time. The first time is decided in
/// one write transaction: the agent registering and the app's window asking for the agent's info
/// (or two processes) each found no id, made one, and the last written won, so the machine was
/// registered under an id it then forgot, and showed twice on the website.
fn install_id_in(store: &crate::store::Store) -> Result<String> {
    use crate::store::settings::{INSTALL_ID, get, set};
    let kept = |conn: &rusqlite::Connection| -> Result<Option<String>> { Ok(get::<String>(conn, INSTALL_ID)?.filter(|id| !id.trim().is_empty())) };
    if let Some(id) = store.read(kept)? {
        return Ok(id);
    }
    store.write(|tx| {
        // Made by another caller while this one waited.
        if let Some(id) = kept(tx)? {
            return Ok(id);
        }
        let id = random_hex();
        set(tx, INSTALL_ID, Some(&id))?;
        Ok(id)
    })
}

/// 16 random bytes as hex: install ids, co-location tokens and nonces.
pub(super) fn random_hex() -> String {
    crate::platform::random_hex(16)
}

/// A human label for this machine (editable server-side later). Not an identifier.
pub(super) fn machine_name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).ok();
    machine_name_from(std::env::var("CYBERCTF_AGENT_NAME").ok(), user, isothrum_runner::this_host())
}

/// The machine label from an explicit name, else the user and the OS.
fn machine_name_from(explicit: Option<String>, user: Option<String>, os: HostOs) -> String {
    if let Some(n) = explicit
        && !n.trim().is_empty()
    {
        return n.trim().to_string();
    }
    let user = user.filter(|s| !s.trim().is_empty());
    let os = match os {
        HostOs::Macos => "Mac",
        HostOs::Windows => "PC",
        HostOs::Linux => "Linux",
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

    /// A real race (Clean Code ch.13, check-then-act): callers asking for the id at once on a
    /// first start each made their own. Many threads released together all get one id.
    #[test]
    fn callers_at_once_on_a_first_start_all_get_one_id() {
        let t = crate::testing::test_app::TestApp::new();
        let start = std::sync::Arc::new(std::sync::Barrier::new(16));
        let callers: Vec<_> = (0..16)
            .map(|_| {
                let (dirs, start) = (t.dirs(), std::sync::Arc::clone(&start));
                std::thread::spawn(move || {
                    start.wait();
                    install_id(&dirs).unwrap()
                })
            })
            .collect();
        let ids: std::collections::BTreeSet<String> = callers.into_iter().map(|c| c.join().unwrap()).collect();
        assert_eq!(ids.len(), 1, "{ids:?}");
        assert_eq!(ids.first(), Some(&install_id(&t.dirs()).unwrap()), "the id kept is the one handed out");
    }

    #[test]
    fn the_install_id_is_made_once_and_kept() {
        let t = crate::testing::test_app::TestApp::new();
        let first = install_id(&t.dirs()).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(install_id(&t.dirs()).unwrap(), first);
        let store = crate::store::open(&t.dirs()).unwrap();
        store.write(|tx| crate::store::settings::set(tx, crate::store::settings::INSTALL_ID, Some(&"   "))).unwrap();
        let regenerated = install_id(&t.dirs()).unwrap();
        assert_eq!(regenerated.len(), 32, "a blank id is none");
        assert!(!t.data().join("install-id").exists(), "no file any more");
    }

    #[test]
    fn the_machine_name_prefers_the_explicit_name_then_the_user() {
        assert_eq!(machine_name_from(Some("  Lab box ".into()), Some("ada".into()), HostOs::Linux), "Lab box");
        assert_eq!(machine_name_from(Some("  ".into()), Some("ada".into()), HostOs::Macos), "ada's Mac");
        assert_eq!(machine_name_from(None, Some("bob".into()), HostOs::Windows), "bob's PC");
        assert_eq!(machine_name_from(None, Some(" ".into()), HostOs::Linux), "Cyber CTF Linux");
        assert_eq!(machine_name_from(None, None, HostOs::Macos), "Cyber CTF Mac");
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
