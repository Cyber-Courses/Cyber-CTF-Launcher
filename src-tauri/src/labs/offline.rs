//! Starting an installed lab again while CyberBackend can't be reached (no network, an outage,
//! a session that can't be refreshed for want of a network): the source of the lab's last
//! `startLab` answer is kept, and replayed when the player asks to start offline and the copy
//! installed here is that commit. Nothing is downloaded offline.
//!
//! Only what runs the lab is kept: the lab, its runtime, repository and commit. Not the launch
//! token, nor the API URL: offline the lab's `evidence` service can't reach the backend anyway,
//! and a token kept on disk would outlive a sign-out. So an offline start has no evidence and no
//! progress tracking, and a backend that answers (even with a refusal) is never bypassed: only a
//! backend that can't be reached is ([`Error::Unreachable`]). The answer is kept for the account
//! it was given to: a signed-out player, or another account, gets no offline start. Cloud labs
//! never start offline: they run in a cloud account, which needs the network anyway.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::runtime::{self, Runtime};

/// What a launch that failed for want of the backend says when the lab can start offline. The app
/// knows it by "can start offline" (lab-operations.ts) and asks the player.
pub const OFFLINE_READY: &str = "This lab is installed here, so it can start offline, without saving progress.";

/// The first line of an offline start's log.
pub const STARTING_OFFLINE: &str = "Starting offline: progress and evidence aren't saved until the lab is started online again.";

/// The source of a lab's last `startLab` answer.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Kept {
    lab_id: String,
    runtime: Runtime,
    repository: String,
    commit: String,
    /// The account `startLab` answered (see `auth::signed_in_user`).
    #[serde(default)]
    user: String,
}

/// The launch spec an offline start runs: the source, no environment.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Spec<'a> {
    lab_id: &'a str,
    runtime: Runtime,
    repository: &'a str,
    commit: &'a str,
    env: [(); 0],
}

/// Where lab `lab_id`'s kept source is, under the app data folder `base`.
fn kept_path(base: &Path, lab_id: &str) -> PathBuf {
    base.join("offline").join(format!("{lab_id}.json"))
}

/// Keeps the source of a `startLab` answer (`launch`) to account `user` for an offline start
/// later, this OS user's only. The answer's environment (the launch token) is never written.
pub fn keep(base: &Path, launch: &Value, user: &str) -> Result<()> {
    let mut kept: Kept = serde_json::from_value(launch.clone()).map_err(|e| Error::Invalid(format!("invalid launch spec: {e}")))?;
    kept.user = user.to_string();
    runtime::validate_id(&kept.lab_id)?;
    super::validate_source(&kept.repository, &kept.commit)?;
    let path = kept_path(base, &kept.lab_id);
    if let Some(dir) = path.parent() {
        crate::private_fs::dir(dir)?;
    }
    let raw = serde_json::to_string(&kept).map_err(|e| Error::Invalid(e.to_string()))?;
    crate::private_fs::write(&path, raw)?;
    Ok(())
}

/// The launch spec that starts lab `lab_id` offline for account `user`: its kept source with no
/// environment, when it was kept for that account and the copy installed here is that commit.
fn offline_spec(base: &Path, lab_id: &str, user: Option<&str>) -> Result<Value> {
    runtime::validate_id(lab_id)?;
    let kept: Kept = std::fs::read_to_string(kept_path(base, lab_id))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(|k: &Kept| k.lab_id == lab_id && !k.user.is_empty() && Some(k.user.as_str()) == user)
        .ok_or_else(|| Error::Invalid("This lab was never started on this machine while online with this account, so it can't start offline.".into()))?;
    if kept.runtime == Runtime::Cloud {
        return Err(Error::Invalid("A cloud lab runs in your cloud account: it can't start offline.".into()));
    }
    if !super::installed(&base.join("labs").join(lab_id).join(".cyberctf-commit"), &kept.commit) {
        return Err(Error::Invalid("The copy of this lab installed here isn't the one last started online, so it can't start offline.".into()));
    }
    let spec = Spec { lab_id: &kept.lab_id, runtime: kept.runtime, repository: &kept.repository, commit: &kept.commit, env: [] };
    serde_json::to_value(spec).map_err(|e| Error::Invalid(e.to_string()))
}

/// What a launch does when `startLab` failed with `failed`: a backend that answered (a refusal,
/// signed out) is the answer. One that couldn't be reached starts the lab offline for account
/// `user` when the player asked to (`offline`) and it can, else says that it can.
pub fn instead_of(failed: Error, base: &Path, lab_id: &str, user: Option<&str>, offline: bool) -> Result<Value> {
    let Error::Unreachable(why) = failed else { return Err(failed) };
    match offline_spec(base, lab_id, user) {
        Ok(spec) if offline => Ok(spec),
        Ok(_) => Err(Error::Unreachable(format!("{why}. {OFFLINE_READY}"))),
        Err(not_ready) if offline => Err(not_ready),
        Err(_) => Err(Error::Unreachable(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const ADA: &str = "user-ada";

    fn base() -> PathBuf {
        std::env::temp_dir().join(format!("cyberctf-offline-{}", rand::random::<u32>()))
    }

    fn start_lab_answer(lab: &str) -> Value {
        json!({
            "labId": lab, "runtime": "DOCKER", "repository": "CyberCTF/web", "commit": SHA,
            "env": [{ "name": "CTF_API_URL", "value": "https://api" }, { "name": "CTF_LAUNCH_TOKEN", "value": "secret-token" }]
        })
    }

    fn install(base: &Path, lab: &str, commit: &str) {
        let dir = base.join("labs").join(lab);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".cyberctf-commit"), commit).unwrap();
    }

    fn unreachable() -> Error {
        Error::Unreachable("API unreachable: connection refused".into())
    }

    #[test]
    fn the_kept_answer_has_no_launch_token_and_is_this_users_only() {
        let base = base();
        keep(&base, &start_lab_answer("web"), ADA).unwrap();
        let path = kept_path(&base, "web");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("secret-token") && !raw.contains("CTF_") && !raw.contains("https://api"), "{raw}");
        assert!(raw.contains(ADA));
        #[cfg(unix)]
        {
            assert_eq!(crate::private_fs::tests::mode(&path), 0o600);
            assert_eq!(crate::private_fs::tests::mode(path.parent().unwrap()), 0o700);
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn an_unreachable_backend_offers_then_starts_an_installed_lab_offline() {
        let base = base();
        keep(&base, &start_lab_answer("web"), ADA).unwrap();
        install(&base, "web", SHA);
        let offered = instead_of(unreachable(), &base, "web", Some(ADA), false).unwrap_err();
        assert!(matches!(offered, Error::Unreachable(_)));
        assert_eq!(offered.to_string(), format!("API unreachable: connection refused. {OFFLINE_READY}"));
        let spec = instead_of(unreachable(), &base, "web", Some(ADA), true).unwrap();
        assert_eq!(spec, json!({ "labId": "web", "runtime": "DOCKER", "repository": "CyberCTF/web", "commit": SHA, "env": [] }));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_backend_that_answers_is_never_bypassed() {
        let base = base();
        keep(&base, &start_lab_answer("web"), ADA).unwrap();
        install(&base, "web", SHA);
        for answered in [Error::Invalid("You don't have access to this lab.".into()), Error::Invalid(crate::account::auth::SIGNED_OUT.into())] {
            let message = answered.to_string();
            let err = instead_of(answered, &base, "web", Some(ADA), true).unwrap_err();
            assert_eq!(err.to_string(), message);
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn no_offline_start_without_a_kept_answer_or_with_another_version_installed() {
        let base = base();
        // Never started online here: the failure as it was, and no offline start.
        install(&base, "web", SHA);
        assert_eq!(instead_of(unreachable(), &base, "web", Some(ADA), false).unwrap_err().to_string(), "API unreachable: connection refused");
        assert!(
            instead_of(unreachable(), &base, "web", Some(ADA), true)
                .unwrap_err()
                .to_string()
                .contains("never started on this machine while online with this account")
        );
        // Another version installed (or none): nothing would run without a download.
        keep(&base, &start_lab_answer("web"), ADA).unwrap();
        install(&base, "web", "ffffffffffffffffffffffffffffffffffffffff");
        assert!(!instead_of(unreachable(), &base, "web", Some(ADA), false).unwrap_err().to_string().contains(OFFLINE_READY));
        assert!(instead_of(unreachable(), &base, "web", Some(ADA), true).unwrap_err().to_string().contains("isn't the one last started online"));
        // Another lab's kept answer never starts this one, and a hostile id is refused.
        keep(&base, &start_lab_answer("other"), ADA).unwrap();
        assert!(instead_of(unreachable(), &base, "../web", Some(ADA), true).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn only_the_account_that_started_it_online_starts_it_offline() {
        let base = base();
        keep(&base, &start_lab_answer("web"), ADA).unwrap();
        install(&base, "web", SHA);
        for user in [None, Some("user-bob")] {
            assert_eq!(instead_of(unreachable(), &base, "web", user, false).unwrap_err().to_string(), "API unreachable: connection refused");
            assert!(instead_of(unreachable(), &base, "web", user, true).unwrap_err().to_string().contains("with this account"));
        }
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_cloud_lab_never_starts_offline() {
        let base = base();
        let mut cloud = start_lab_answer("web");
        cloud["runtime"] = json!("CLOUD");
        keep(&base, &cloud, ADA).unwrap();
        install(&base, "web", SHA);
        assert!(!instead_of(unreachable(), &base, "web", Some(ADA), false).unwrap_err().to_string().contains(OFFLINE_READY));
        assert!(instead_of(unreachable(), &base, "web", Some(ADA), true).unwrap_err().to_string().contains("can't start offline"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_launch_answer_from_elsewhere_is_never_kept() {
        let base = base();
        let mut foreign = start_lab_answer("web");
        foreign["repository"] = json!("someone/web");
        assert!(keep(&base, &foreign, ADA).is_err());
        assert!(keep(&base, &start_lab_answer("../x"), ADA).is_err());
        assert!(!kept_path(&base, "web").exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
