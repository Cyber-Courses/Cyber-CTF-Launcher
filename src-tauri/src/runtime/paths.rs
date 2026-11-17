//! Where a lab's files live (its folder, the runner's home), and the one marker file the launcher
//! still leaves beside them: the Docker engine a container lab was started on. Where and in what
//! form a lab runs, and whether it is parked, the runner's record of its run says (`recorded`).

use std::path::{Path, PathBuf};

use crate::platform::AppDirs;

use crate::error::{Error, Result};

/// Whether `id` is a lab id: non-empty, at most 64 characters, and only the characters an id is
/// built from (ASCII letters and digits, `-`, `_`). The one rule for every lab id the launcher
/// reads: a path segment under the labs folder, a worker's key, a project or VM name's part. It
/// keeps `..`, a separator or a stray token (an attack box's `cyberctf-<id>-attack`) out.
pub(crate) fn is_lab_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `id` when it is a lab id ([`is_lab_id`]), else the error naming it: for an id that builds a
/// path or a command.
///
/// # Errors
///
/// Not a lab id.
pub fn validate_id(id: &str) -> Result<()> {
    if is_lab_id(id) { Ok(()) } else { Err(Error::Invalid(format!("invalid lab id `{id}`"))) }
}

/// The Isothrum runner's home: a folder of its own in the app data folder (`isothrum/`), whose
/// files are the runner's alone (its store, its runs' folders, its SSH key, its Terraform plugin
/// cache). The launcher reads none of them: it asks the runner.
pub(crate) fn runner_home(dirs: &AppDirs) -> Result<PathBuf> {
    Ok(runner_home_in(dirs.data()?))
}

/// [`runner_home`] under the app data folder `base`.
pub(crate) fn runner_home_in(base: &Path) -> PathBuf {
    base.join("isothrum")
}

/// Where installed labs live: `<app data>/labs/`, each in `<lab id>/`.
///
/// # Errors
///
/// No data folder.
pub(crate) fn labs_root(dirs: &AppDirs) -> Result<PathBuf> {
    Ok(labs_root_in(dirs.data()?))
}

/// [`labs_root`] under the app data folder `base`.
pub(crate) fn labs_root_in(base: &Path) -> PathBuf {
    base.join("labs")
}

/// An installed lab's folder.
pub(crate) fn lab_dir(dirs: &AppDirs, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    lab_dir_in(dirs.data()?, id)
}

/// [`lab_dir`], none when the lab isn't installed; an invalid id, or no data folder, is still an
/// error.
pub(crate) fn installed_lab_dir(dirs: &AppDirs, id: &str) -> Result<Option<PathBuf>> {
    validate_id(id)?;
    Ok(installed_in(dirs.data()?, id))
}

/// [`lab_dir`] under the app data folder `base` (the id already validated).
pub(super) fn lab_dir_in(base: &Path, id: &str) -> Result<PathBuf> {
    installed_in(base, id).ok_or_else(|| Error::Invalid(format!("lab `{id}` is not installed")))
}

/// Lab `id`'s folder under the app data folder `base`, when it is installed.
fn installed_in(base: &Path, id: &str) -> Option<PathBuf> {
    Some(labs_root_in(base).join(id)).filter(|dir| dir.is_dir())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_lab_id_is_a_bounded_plain_name() {
        for id in ["web", "goad-light_2", "local-invoice", "A", &"a".repeat(64)] {
            assert!(super::is_lab_id(id) && super::validate_id(id).is_ok(), "{id}");
        }
        for id in ["", "../x", "a/b", "a b", "lab.1", "é", &"a".repeat(65)] {
            assert!(!super::is_lab_id(id), "{id}");
            assert_eq!(super::validate_id(id).unwrap_err().to_string(), format!("invalid lab id `{id}`"));
        }
    }

    use super::super::Runtime;
    use super::{lab_dir_in, validate_id};

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_runtimes_tag_is_its_serde_name_both_ways() {
        // One spelling: the tag the launcher writes and reads is what CyberBackend and the
        // webview send.
        for r in [Runtime::Docker, Runtime::Vm, Runtime::Cloud] {
            assert_eq!(serde_json::to_value(r).unwrap(), r.tag());
            assert_eq!(serde_json::from_value::<Runtime>(r.tag().into()).unwrap(), r);
        }
        assert!(serde_json::from_value::<Runtime>("vm".into()).is_err());
    }

    #[test]
    fn rejects_path_traversal_and_shell_characters() {
        for bad in ["", "../x", "a/b", "a b", "a;rm", "é", &"x".repeat(65)] {
            assert!(validate_id(bad).is_err(), "{bad:?} should be rejected");
        }
        for good in ["web-1", "sqli_basic", "3f2a9c1e-7b1d-4c4e-9a53-2d1c8f0e6b7a"] {
            assert!(validate_id(good).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn a_lab_folder_must_be_installed() {
        let base = temp_dir("cyberctf-appdata");
        let err = lab_dir_in(&base, "web-1").unwrap_err().to_string();
        assert_eq!(err, "lab `web-1` is not installed");
        std::fs::create_dir_all(base.join("labs").join("web-1")).unwrap();
        assert_eq!(lab_dir_in(&base, "web-1").unwrap(), base.join("labs").join("web-1"));
        std::fs::remove_dir_all(base).unwrap();
    }
}
