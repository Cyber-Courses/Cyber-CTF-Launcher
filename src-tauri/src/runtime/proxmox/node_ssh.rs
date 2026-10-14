//! SSH to the Proxmox node: its login, and letting the launcher's key in so labs on an internal
//! bridge can be reached through it.

use super::*;

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
    let askpass = write_askpass(identity)?;
    let env = vec![
        ("SSH_ASKPASS".to_string(), askpass.display().to_string()),
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("DISPLAY".to_string(), ":0".to_string()),
        ("CYBERCTF_SSH_PASSWORD".to_string(), password.to_string()),
    ];
    let args = authorize_args(h, identity, public);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::exec::run_env_timed("ssh", &refs, None, &env, AUTHORIZE_TIMEOUT).await.map(|_| ())
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
    let known_hosts = ssh::node_known_hosts(identity).display().to_string();
    let password_auth =
        ["PreferredAuthentications=password,keyboard-interactive".to_string(), "NumberOfPasswordPrompts=1".to_string(), "ConnectTimeout=10".to_string()];
    let mut args: Vec<String> = Vec::new();
    for o in password_auth.into_iter().chain(ssh::options(false, &known_hosts)) {
        args.extend(["-o".into(), o]);
    }
    // `--`: the login comes from the host profile, so it can never be read as an ssh option
    // (a host such as `-oProxyCommand=...`).
    args.extend(["--".into(), node_login(h), command]);
    args
}

/// The SSH user for the snippet upload: the token's user without its realm.
pub fn ssh_user(username: &str) -> &str {
    token_user(username).split('@').next().unwrap_or("root")
}

/// Token setups upload the snippet over SSH with the launcher's key: checks it's authorized.
pub(super) async fn ssh_key_problem(h: &HostProfile) -> Option<String> {
    let Some(identity) = ssh::launcher_key() else {
        return Some("The launcher's SSH key isn't ready yet; test again.".into());
    };
    let public = std::fs::read_to_string(identity.with_extension("pub")).unwrap_or_default().trim().to_string();
    let user = ssh_user(&h.username).to_string();
    let target = ssh::Target::direct(&h.host, &user, identity.clone());
    match target.exec_timed(&ssh::node_known_hosts(&identity), "true", std::time::Duration::from_secs(30)).await {
        Ok(_) => None,
        Err(_) => Some(format!(
            "The launcher can't SSH to {user}@{} with its key, which a token setup needs to upload the lab's cloud-init. On the node, run:\n  echo '{public}' >> ~/.ssh/authorized_keys",
            h.host
        )),
    }
}
