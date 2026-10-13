//! The command lines for a lab's shells: the attack box wherever the lab runs (the local
//! container, or over SSH on the lab host or local lab VM), and the attack VM beside a VM lab.

use crate::AppHandle;

use super::paths::{lab_dir, local_vm, state_dir};
use super::{Runtime, attack_vm, exegol, lab, remote, server, ssh};
use crate::error::{Error, Result};

/// Which shell an embedded terminal attaches to.
#[derive(Debug, Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ShellKind {
    /// The lab's attack box, wherever the lab runs.
    Lab,
    /// The attack VM beside a VM lab on this machine.
    AttackVm,
}

/// The command line for a lab's attack box shell, wherever the lab runs, and, for an in-app
/// shell (`tag`) in the local attack box container, that container (whose marked processes end
/// with the shell).
pub(super) async fn lab_shell_command(app: &AppHandle, id: &str, runtime: Runtime, tag: Option<&str>) -> Result<(String, Option<String>)> {
    let dir = lab_dir(app, id)?;
    if runtime == Runtime::Cloud {
        return Err(Error::Invalid("A cloud lab has no attack box: work from your own terminal, with what the lab gives you.".into()));
    }
    let target = match server::lab_connection(app, &dir)? {
        None if runtime == Runtime::Docker && local_vm(&dir).is_some() => remote::vagrant_host(&lab::vagrant_dir(&dir, runtime), &[], "lab VM").await?,
        // The local attack box container (`lab_dir` has validated the id it is named after).
        None => {
            return Ok(match tag {
                Some(tag) => (exegol::tagged_shell_command(id, tag), Some(exegol::container(id))),
                None => (exegol::shell_command(id), None),
            });
        }
        Some(_) if runtime != Runtime::Docker => return Err(Error::Invalid("this lab has no attack box".into())),
        Some(conn) => match server::terraform_target(conn.provider) {
            Some(tf) => remote::terraform_host(app, &state_dir(app, id, tf)?)
                .await?
                .ok_or_else(|| Error::Invalid("the lab host has no address yet; wait for it to finish starting".into()))?,
            None => remote::vagrant_host(&lab::vagrant_dir(&dir, runtime), &conn.env, "lab host").await?,
        },
    };
    Ok((target.attack_shell_command(&ssh::known_hosts(app)?)?, None))
}

/// The command line an embedded terminal runs for `kind`, and the attack box container whose
/// processes marked with `tag` end with it (see [`end_shell_session`]).
pub async fn shell_command_for(app: &AppHandle, id: &str, kind: ShellKind, runtime: Runtime, tag: &str) -> Result<(String, Option<String>)> {
    match kind {
        ShellKind::Lab => lab_shell_command(app, id, runtime, Some(tag)).await,
        ShellKind::AttackVm => Ok((attack_vm::shell_command(&lab_dir(app, id)?)?, None)),
    }
}

/// The folder an embedded terminal runs the attack VM's `vagrant ssh` in, and the machine (on
/// Windows, which has no `sh` for the command line, vagrant runs there directly).
pub fn attack_vm_shell_target(app: &AppHandle, id: &str) -> Result<(std::path::PathBuf, Option<&'static str>)> {
    attack_vm::shell_target(&lab_dir(app, id)?)
}

/// Ends what an in-app shell left running in the attack box container.
pub async fn end_shell_session(container: &str, tag: &str) {
    exegol::end_session(container, tag).await
}

/// [`end_shell_session`], blocking (the app is quitting).
pub fn end_shell_session_blocking(container: &str, tag: &str) {
    exegol::end_session_blocking(container, tag)
}

/// An attack-box image reference the launcher accepts.
pub fn valid_image(image: &str) -> bool {
    exegol::valid_image(image)
}

#[cfg(test)]
mod tests {
    use super::{ShellKind, valid_image};

    #[test]
    fn the_ui_names_the_shell_kinds_in_camel_case() {
        assert!(matches!(serde_json::from_str::<ShellKind>("\"lab\"").unwrap(), ShellKind::Lab));
        assert!(matches!(serde_json::from_str::<ShellKind>("\"attackVm\"").unwrap(), ShellKind::AttackVm));
        assert!(serde_json::from_str::<ShellKind>("\"attack_vm\"").is_err());
    }

    #[test]
    fn attack_box_images_are_checked_like_the_commands_check_them() {
        assert!(valid_image("nwodtuhs/exegol:free"));
        assert!(!valid_image("-x"));
    }
}
