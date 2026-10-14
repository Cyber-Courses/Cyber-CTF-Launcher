//! Reaching the host a lab runs on over SSH: a Terraform lab host (server or cloud), or a lab VM
//! Vagrant knows (local, or on ESXi). Shared by the attack box, the lab shell and the status probe.

use std::path::Path;

use crate::AppHandle;

use super::{ssh, terraform};
use crate::error::{Error, Result};

/// The SSH target of a Terraform lab host, with the launcher's key. None while the state has no
/// address yet.
pub(super) async fn terraform_host(app: &AppHandle, state: &Path) -> Result<Option<ssh::Target>> {
    // `ensure_key`, not `launcher_key`: right after the app starts, nothing has loaded the key yet.
    Ok(terraform::ssh_target(state, ssh::ensure_key(app).await?.0))
}

/// The SSH target of a lab VM, from `vagrant ssh-config` in its vagrant folder; `what` names it
/// in the error ("lab VM", "lab host").
pub(super) async fn vagrant_host(vagrant: &Path, env: &[(String, String)], what: &str) -> Result<ssh::Target> {
    let out = crate::exec::run_env("vagrant", &["ssh-config"], Some(vagrant), env).await?;
    ssh::parse_ssh_config(&out).ok_or_else(|| Error::Invalid(format!("couldn't read the {what}'s SSH settings")))
}

/// `script` as one root shell command on a lab host.
pub(super) fn sudo_bash(script: &str) -> String {
    format!("sudo bash -c {}", ssh::sh_quote(script))
}

/// Runs `script` as root on a Terraform lab host, trusting the host keys kept with its state.
pub(super) async fn run_on_terraform_host(target: &ssh::Target, state: &Path, script: &str) -> Result<String> {
    target.exec(&terraform::known_hosts(state), &sudo_bash(script)).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn sudo_bash_runs_the_script_as_one_quoted_argument() {
        let cmd = super::sudo_bash("echo 'hi' && id");
        assert!(cmd.starts_with("sudo bash -c "));
        assert_eq!(cmd, format!("sudo bash -c {}", super::ssh::sh_quote("echo 'hi' && id")));
    }
}
