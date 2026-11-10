//! SSH to the Proxmox node: its login, and letting the launcher's key in so labs on an internal
//! bridge can be reached through it.

use super::{HostProfile, ssh, token_user};

/// The node's SSH login (`user@host`) for going through it.
pub fn node_login(h: &HostProfile) -> String {
    format!("{}@{}", ssh_user(&h.username), h.host)
}

/// Lets the launcher's key into the node's SSH (password hosts; a token setup already needs
/// it, see `ssh_key_problem`), so labs on an internal bridge can be reached through the node.
/// Uses the stored password once, through SSH's own askpass hook; idempotent.
pub async fn authorize_launcher_key(h: &HostProfile, password: &str, identity: &std::path::Path, public: &str) -> crate::error::Result<()> {
    // Next to the launcher's key (the player's own app data), not the shared system temp dir:
    // on Linux /tmp is common to every account, and a file another user left there under this
    // name can't be rewritten (or could be swapped for something else).
    let env = password_env(identity, password)?;
    ssh_with(&authorize_args(h, identity, public), &env, AUTHORIZE_TIMEOUT).await.map(|_| ())
}

/// The most letting the key in may take: a node that stops answering must not hang the launch.
const AUTHORIZE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The askpass helper's file name and contents: it answers SSH's prompt with the password in
/// `CYBERCTF_SSH_PASSWORD`. Windows' OpenSSH can't run a `sh` script: there it is a `.cmd`, the
/// password echoed after the line is parsed (delayed expansion), so `&`, `|` or `%` in it are
/// printed, not run.
pub(super) fn askpass_script(windows: bool) -> (&'static str, &'static str) {
    if windows {
        ("askpass.cmd", "@echo off\r\nsetlocal EnableDelayedExpansion\r\necho(!CYBERCTF_SSH_PASSWORD!\r\n")
    } else {
        ("askpass.sh", "#!/bin/sh\nprintf '%s\\n' \"$CYBERCTF_SSH_PASSWORD\"\n")
    }
}

/// SSH's environment for a password login: its askpass hook (a script next to the launcher's key
/// `identity`) answering once with `password`.
fn password_env(identity: &std::path::Path, password: &str) -> crate::error::Result<Vec<(String, String)>> {
    let askpass = write_askpass(identity)?;
    Ok(vec![
        ("SSH_ASKPASS".to_string(), askpass.display().to_string()),
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("DISPLAY".to_string(), ":0".to_string()),
        ("CYBERCTF_SSH_PASSWORD".to_string(), password.to_string()),
    ])
}

/// `ssh` with `args` and `env`, for at most `timeout`: its output.
async fn ssh_with(args: &[String], env: &[(String, String)], timeout: std::time::Duration) -> crate::error::Result<String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::platform::exec::run_env_timed("ssh", &refs, None, env, timeout).await
}

/// The askpass helper next to the launcher's key (see [`askpass_script`]), this user's only.
pub(super) fn write_askpass(identity: &std::path::Path) -> crate::error::Result<std::path::PathBuf> {
    let (name, script) = askpass_script(cfg!(windows));
    let askpass = identity.with_file_name(name);
    std::fs::write(&askpass, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(askpass)
}

/// The `ssh` arguments that add `public` to the node's authorized keys, signing in by password.
pub(super) fn authorize_args(h: &HostProfile, identity: &std::path::Path, public: &str) -> Vec<String> {
    let key = ssh::sh_quote(public.trim());
    let command = format!("umask 077; mkdir -p ~/.ssh; grep -qxF {key} ~/.ssh/authorized_keys 2>/dev/null || echo {key} >> ~/.ssh/authorized_keys");
    password_ssh_args(h, identity, &command)
}

/// The `ssh` arguments that run `command` on the node, signing in by password (through the
/// askpass helper next to `identity`).
pub(super) fn password_ssh_args(h: &HostProfile, identity: &std::path::Path, command: &str) -> Vec<String> {
    let known_hosts = ssh::node_known_hosts(identity).display().to_string();
    let password_auth =
        ["PreferredAuthentications=password,keyboard-interactive".to_string(), "NumberOfPasswordPrompts=1".to_string(), "ConnectTimeout=10".to_string()];
    let mut args: Vec<String> = Vec::new();
    for o in password_auth.into_iter().chain(ssh::options(false, &known_hosts)) {
        args.extend(["-o".into(), o]);
    }
    // `--`: the login comes from the host profile, so it can never be read as an ssh option
    // (a host such as `-oProxyCommand=...`).
    args.extend(["--".into(), node_login(h), command.to_string()]);
    args
}

/// Whether the node's `/etc/network/interfaces` sources the SDN configuration (`interfaces.d/*`
/// or `interfaces.d/sdn`): read-only, it changes nothing on the node.
pub(super) const SDN_SOURCE_CHECK: &str =
    r"grep -Eq '^[[:space:]]*source[[:space:]]+/etc/network/interfaces\.d/(\*|sdn)([[:space:]]|$)' /etc/network/interfaces";

/// What's wrong, and Proxmox's documented fix, when the node doesn't source its SDN config.
pub(super) const SDN_MISSING: &str = "The node's /etc/network/interfaces doesn't load the SDN configuration, so a lab's network (an SDN vnet bridge) is never created and its VMs fail to start with \"bridge ... does not exist\". On the node, add this line at the end of /etc/network/interfaces:\n  source /etc/network/interfaces.d/*\nthen run: ifreload -a";

/// The problem a launch would hit when the node doesn't source its SDN config (checked over SSH:
/// the launcher's key `identity` on token setups, the password otherwise, its askpass helper and
/// the nodes' known hosts beside that key). A token setup whose SSH fails is reported by
/// `ssh_key_problem` already.
pub(super) async fn sdn_problem(h: &HostProfile, password: &str, identity: &std::path::Path) -> Option<String> {
    let known_hosts = ssh::node_known_hosts(identity);
    let timeout = std::time::Duration::from_secs(30);
    let token = super::is_token(&h.username);
    let checked = if token {
        ssh::Target::direct(&h.host, ssh_user(&h.username), identity.to_path_buf()).exec_timed(&known_hosts, SDN_SOURCE_CHECK, timeout).await
    } else {
        let env = match password_env(identity, password) {
            Ok(env) => env,
            Err(e) => return Some(format!("Couldn't check the node's network configuration over SSH ({e}).")),
        };
        ssh_with(&password_ssh_args(h, identity, SDN_SOURCE_CHECK), &env, timeout).await
    };
    sdn_outcome(checked, (!token).then(|| node_login(h)))
}

/// `sdn_problem` from what the check over SSH gave: grep found no directive (it fails without a
/// word), or SSH itself failed, reported for a password setup (`password_login`, the login it
/// tried) and left to `ssh_key_problem` for a token one.
pub(super) fn sdn_outcome(checked: crate::error::Result<String>, password_login: Option<String>) -> Option<String> {
    match (checked, password_login) {
        (Err(crate::error::Error::CommandFailed { stderr, .. }), _) if stderr.trim().is_empty() => Some(SDN_MISSING.into()),
        (Err(e), Some(login)) => Some(format!("Couldn't check the node's network configuration over SSH as {login} ({e}).")),
        (Ok(_), _) | (Err(_), None) => None,
    }
}

/// The SSH user for the snippet upload: the token's user without its realm.
pub fn ssh_user(username: &str) -> &str {
    token_user(username).split('@').next().unwrap_or("root")
}

/// Token setups upload the snippet over SSH with the launcher's key `identity`: checks it's
/// authorized.
pub(super) async fn ssh_key_problem(h: &HostProfile, identity: &std::path::Path) -> Option<String> {
    let user = ssh_user(&h.username).to_string();
    let target = ssh::Target::direct(&h.host, &user, identity.to_path_buf());
    if target.exec_timed(&ssh::node_known_hosts(identity), "true", std::time::Duration::from_secs(30)).await.is_ok() {
        return None;
    }
    let public = identity.with_extension("pub");
    Some(key_not_authorized(&user, &h.host, &public, &std::fs::read_to_string(&public)))
}

/// What to do when the node refuses the launcher's key: add it, the command to run on the node
/// written out with the public key read from `public`. A public key that can't be read (or is
/// empty) is said instead: the command would add an empty line.
fn key_not_authorized(user: &str, host: &str, public: &std::path::Path, read: &std::io::Result<String>) -> String {
    let lead = format!("The launcher can't SSH to {user}@{host} with its key, which a token setup needs to upload the lab's cloud-init.");
    match read.as_ref().map(|k| k.trim()) {
        Ok(key) if !key.is_empty() => format!("{lead} On the node, run:\n  echo '{key}' >> ~/.ssh/authorized_keys"),
        Ok(_) => format!("{lead} Its public key ({}) is empty: set the server up again in the launcher to make a new key.", public.display()),
        Err(e) => format!("{lead} Its public key ({}) can't be read ({e}): set the server up again in the launcher to make a new key.", public.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_key_says_the_command_with_the_key_or_why_there_is_none() {
        let path = std::path::Path::new("/keys/id_ed25519.pub");
        let ok = key_not_authorized("root", "pve", path, &Ok("ssh-ed25519 AAAA launcher\n".into()));
        assert!(ok.ends_with("echo 'ssh-ed25519 AAAA launcher' >> ~/.ssh/authorized_keys"), "{ok}");
        // Regression: a public key that couldn't be read gave `echo '' >> ~/.ssh/authorized_keys`.
        for read in [Ok("  \n".to_string()), Err(std::io::Error::from(std::io::ErrorKind::NotFound))] {
            let said = key_not_authorized("root", "pve", path, &read);
            assert!(!said.contains("echo ''") && said.contains("/keys/id_ed25519.pub") && said.contains("set the server up again"), "{said}");
        }
    }
}
