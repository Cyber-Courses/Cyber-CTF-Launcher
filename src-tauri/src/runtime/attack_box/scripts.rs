//! The attack box's set-up scripts, from the attack box's own repository ([`REPOSITORY`]) at the
//! commit the launcher pins (`attack-box.json`, which that repository's publish updates): each
//! downloaded once, checked against its pinned SHA-256 before it is kept (the scripts run as root
//! in the box), then read from the app's cache. The launcher keeps no copy of them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The pin: the attack box's image, the earlier ones, the repository's commit and each script's
/// SHA-256 at it. Shared with the frontend's Settings (`src/lib/settings.ts`).
pub(super) const PIN: &str = include_str!("../../../attack-box.json");

/// Where the scripts are read from: the repository's files at a commit.
const REPOSITORY: &str = "https://raw.githubusercontent.com/CyberCTF/attack-box";

/// The largest script downloaded.
const MAX_SCRIPT: u64 = 1024 * 1024;

/// The scripts the box's set-up is made of, as the attack box repository's README orders them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scripts {
    /// `player-account.sh`: makes the player's account (both forms).
    pub(crate) player_account: String,
    /// `vm/kali.sh`: a Debian VM made Kali, Kali's archive keys checked first.
    pub(crate) kali: String,
    /// `vm/toolset.sh`: Kali's headless toolset, unless the box has it.
    pub(crate) toolset: String,
    /// `vm/desktop.sh`: XFCE and the player signed in, for a box with a desktop.
    pub(crate) desktop: String,
}

#[derive(serde::Deserialize)]
struct Pin {
    commit: String,
    files: BTreeMap<String, String>,
}

fn pin() -> Result<Pin> {
    serde_json::from_str(PIN).map_err(|e| Error::Invalid(format!("the attack box's pin (attack-box.json) doesn't read: {e}")))
}

/// Whether the launcher's tests set up an attack box with the real scripts: the real-lab scenarios
/// do ([`use_the_real_scripts`]), so their box is the player's; unit tests get [`for_tests`] and
/// download nothing. For the whole test process: the scenarios run in a process of their own.
#[cfg(test)]
static REAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Has the attack box set up with the pinned scripts from here on, in this test process.
#[cfg(test)]
pub(crate) fn use_the_real_scripts() {
    REAL.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The scripts at the pinned commit: from the cache (`<cache>/attack-box/<sha256>`) when there,
/// else downloaded, checked and kept there (only checked, when the app's cache folder is unknown).
/// `log` says when a download starts. In the launcher's tests, `for_tests` unless the real ones
/// are asked for (`use_the_real_scripts`): both are test-only, so not links in the docs.
pub(crate) async fn fetch(dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<Scripts> {
    #[cfg(test)]
    if !REAL.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(for_tests());
    }
    let pin = pin()?;
    let cache = dirs.cache().ok().map(|c| c.join("attack-box"));
    let mut read = async |name: &str| -> Result<String> {
        let want = pin.files.get(name).ok_or_else(|| Error::Invalid(format!("the attack box's pin names no {name}")))?;
        if let Some(text) = cache.as_deref().and_then(|c| cached(c, want)) {
            return Ok(text);
        }
        log(format!("Downloading the attack box's {name} (CyberCTF/attack-box at {})", short(&pin.commit)));
        let bytes = download(&format!("{REPOSITORY}/{}/{name}", pin.commit)).await.map_err(|e| {
            Error::Invalid(format!("The attack box's set-up couldn't be downloaded from GitHub ({e}). Check this computer's connection, then try again."))
        })?;
        let text = checked(name, &bytes, want)?;
        if let Some(cache) = &cache {
            keep(cache, want, &bytes);
        }
        Ok(text)
    };
    Ok(Scripts {
        player_account: read("player-account.sh").await?,
        kali: read("vm/kali.sh").await?,
        toolset: read("vm/toolset.sh").await?,
        desktop: read("vm/desktop.sh").await?,
    })
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// The cached file of SHA-256 `want`, when it is there and still holds it.
fn cached(cache: &Path, want: &str) -> Option<String> {
    let bytes = std::fs::read(cache.join(want)).ok()?;
    (hex(&bytes) == want).then(|| String::from_utf8(bytes).ok()).flatten()
}

/// Keeps `bytes` under their hash; a failure only means the next start downloads again.
fn keep(cache: &Path, want: &str, bytes: &[u8]) {
    let path: PathBuf = cache.join(want);
    let kept = std::fs::create_dir_all(cache).and_then(|()| std::fs::write(&path, bytes));
    if let Err(e) = kept {
        log::warn!("couldn't keep the attack box's script in {}: {e}", path.display());
    }
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let res = crate::platform::http::download_client()?.get(url).send().await.map_err(|e| Error::Invalid(e.to_string()))?;
    let res = res.error_for_status().map_err(|e| Error::Invalid(e.to_string()))?;
    crate::platform::http::body_limited(res, MAX_SCRIPT).await
}

/// `bytes` as the script `name`, when their SHA-256 is the pinned `want`.
fn checked(name: &str, bytes: &[u8], want: &str) -> Result<String> {
    let got = hex(bytes);
    if got != want {
        return Err(Error::Invalid(format!(
            "The attack box's {name} from GitHub isn't the one this launcher pins (SHA-256 {got}, expected {want}): nothing was run."
        )));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| Error::Invalid(format!("The attack box's {name} isn't text")))
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Scripts for the launcher's own tests: what each part of the box's set-up is, by its name,
/// without the attack box repository's text (its own checks test that).
#[cfg(test)]
pub(crate) fn for_tests() -> Scripts {
    Scripts {
        player_account: "# the player's account\nset -e\n: \"${account:?}\"\necho \"account $account\"\n".into(),
        kali: "# Kali\nkeep=''\necho kali\n".into(),
        toolset: "echo 'toolset: install -y $keep kali-linux-headless'\n".into(),
        desktop: "echo 'desktop: install -y $keep kali-desktop-xfce lightdm'\n".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_names_a_commit_and_every_script_with_its_sha256() {
        let pin = pin().expect("attack-box.json reads");
        assert!(pin.commit.len() == 40 && pin.commit.bytes().all(|b| b.is_ascii_hexdigit()), "a full commit: {}", pin.commit);
        let names: Vec<&str> = pin.files.keys().map(String::as_str).collect();
        assert_eq!(names, ["player-account.sh", "vm/desktop.sh", "vm/kali.sh", "vm/toolset.sh"]);
        assert!(pin.files.values().all(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())), "{:?}", pin.files);
    }

    #[test]
    fn a_script_is_kept_only_when_its_sha256_is_the_pinned_one() {
        let want = hex(b"echo ok\n");
        assert_eq!(checked("x.sh", b"echo ok\n", &want).unwrap(), "echo ok\n");
        let err = checked("x.sh", b"echo evil\n", &want).unwrap_err().to_string();
        assert!(err.contains("isn't the one this launcher pins") && err.contains("nothing was run"), "{err}");
    }

    #[test]
    fn the_cache_serves_a_script_by_its_hash_and_never_a_changed_one() {
        let dir = crate::testing::temp_dir("attack-box-scripts");
        let want = hex(b"echo ok\n");
        assert_eq!(cached(&dir, &want), None, "nothing kept yet");
        keep(&dir, &want, b"echo ok\n");
        assert_eq!(cached(&dir, &want).as_deref(), Some("echo ok\n"));
        std::fs::write(dir.join(&want), b"echo changed\n").unwrap();
        assert_eq!(cached(&dir, &want), None, "a file changed since is downloaded again");
    }
}
