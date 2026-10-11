//! Reaching a remote lab host over SSH, for "Open shell" on labs that run on a server
//! host or in the cloud: the attack box runs next to the lab there, so the shell is
//! `ssh <lab host> sudo docker exec -it attacker bash`.
//!
//! The launcher has its own key (`<app data>/ssh/id_ed25519`), made once with the
//! system's `ssh-keygen` and installed on lab hosts by cloud-init (`ssh_public_key`).
//! Vagrant targets (ESXi) use Vagrant's own key, read from `vagrant ssh-config`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};
use crate::exec::run;

fn ssh_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("ssh");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The launcher's key path once `ensure_key` has run, for code without an AppHandle
/// (the Terraform driver waiting on a lab host it just installed the key on).
static LAUNCHER_KEY: OnceLock<PathBuf> = OnceLock::new();

pub fn launcher_key() -> Option<PathBuf> {
    LAUNCHER_KEY.get().cloned()
}

#[cfg(test)]
pub fn set_launcher_key_for_test(key: PathBuf) {
    let _ = LAUNCHER_KEY.set(key);
}

/// The launcher's private key, created on first use. Returns (key path, public key line).
pub async fn ensure_key(app: &AppHandle) -> Result<(PathBuf, String)> {
    let key = ssh_dir(app)?.join("id_ed25519");
    if !key.is_file() {
        let path = key.to_string_lossy().to_string();
        run("ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-C", "cyberctf-launcher", "-f", &path], None).await?;
    }
    let _ = LAUNCHER_KEY.set(key.clone());
    let public = std::fs::read_to_string(key.with_extension("pub"))?.trim().to_string();
    Ok((key, public))
}

/// Where to SSH: what `ssh` needs, nothing more.
#[derive(Debug, PartialEq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub identity: PathBuf,
    /// `user@host` to go through (with the same key) when `host` isn't routable from here: a
    /// lab VM on a Proxmox node's host-internal bridge, reached via the node.
    pub jump: Option<String>,
}

/// Target from `vagrant ssh-config` output (HostName, Port, User, IdentityFile).
pub fn parse_ssh_config(out: &str) -> Option<Target> {
    let mut host = None;
    let mut port = 22;
    let mut user = None;
    let mut identity = None;
    for line in out.lines() {
        let mut parts = line.trim().splitn(2, char::is_whitespace);
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else { continue };
        let value = value.trim().trim_matches('"');
        match key {
            "HostName" => host = Some(value.to_string()),
            "Port" => port = value.parse().unwrap_or(22),
            "User" => user = Some(value.to_string()),
            // The first IdentityFile is the machine's private key.
            "IdentityFile" if identity.is_none() => identity = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    Some(Target { host: host?, port, user: user?, identity: identity?, jump: None })
}

/// A user or host we are willing to put on a command line.
fn safe_token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 253 && s.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:".contains(c)) && !s.starts_with('-')
}

/// POSIX single-quoting.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A path as the value of an ssh `-o Key=value` option. OpenSSH splits option values into words
/// (on whitespace, honouring quotes and backslash escapes) and expands `%` tokens in file paths:
/// unescaped, `~/Library/Application Support/…/known_hosts` became two files and ssh pinned host
/// keys in `~/Library/Application`. Escaped here so ssh reads the path back verbatim (a tab can't
/// be escaped this way; app paths have none).
pub fn ssh_config_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '\\' | '"' | '\'' | ' ' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out
}

/// The `-o` values every launcher SSH uses: accept a new host's key once, pinned in the
/// launcher's own `known_hosts` (never the player's ~/.ssh), quiet. `batch` adds no prompts and
/// a short connect timeout, and keepalives so a host that stops answering mid-command (a VM
/// that froze, a dropped network) ends the command in under a minute instead of hanging it for
/// hours. `known_hosts` is the plain path; it is escaped for ssh here.
pub fn options(batch: bool, known_hosts: &str) -> Vec<String> {
    let mut opts: Vec<String> = Vec::new();
    if batch {
        opts.extend(["BatchMode=yes".into(), "ConnectTimeout=10".into(), "ServerAliveInterval=15".into(), "ServerAliveCountMax=3".into()]);
    }
    opts.extend(["StrictHostKeyChecking=accept-new".into(), format!("UserKnownHostsFile={}", ssh_config_path(known_hosts)), "LogLevel=ERROR".into()]);
    opts
}

/// A word for a shell command line: as is when it is plain, single-quoted otherwise.
fn shell_word(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "=.,_/:@+-".contains(c));
    if plain { s.to_string() } else { sh_quote(s) }
}

/// `options` as `-o …` words for a shell command line.
fn shell_options(batch: bool, known_hosts: &Path) -> String {
    options(batch, &known_hosts.to_string_lossy()).iter().map(|o| format!("-o {}", shell_word(o))).collect::<Vec<_>>().join(" ")
}

impl Target {
    /// A host reached directly on port 22.
    pub fn direct(host: impl Into<String>, user: impl Into<String>, identity: PathBuf) -> Self {
        Target { host: host.into(), port: 22, user: user.into(), identity, jump: None }
    }

    fn check_endpoint(&self) -> Result<()> {
        if !safe_token(&self.host) || !safe_token(&self.user) {
            return Err(Error::Invalid("unexpected SSH host or user".into()));
        }
        Ok(())
    }

    /// `-o ProxyCommand=…` through the jump host, with the same key and known_hosts.
    fn proxy_option(&self, known_hosts: &Path) -> Result<Option<String>> {
        let Some(jump) = &self.jump else { return Ok(None) };
        let (user, host) = jump.split_once('@').ok_or_else(|| Error::Invalid("unexpected jump host".into()))?;
        if !safe_token(user) || !safe_token(host) {
            return Err(Error::Invalid("unexpected jump host".into()));
        }
        // ssh expands `%` tokens in a ProxyCommand before its shell runs it: only `%h:%p` is ours.
        let inner = format!("ssh -i {} {}", sh_quote(&self.identity.to_string_lossy()), shell_options(true, known_hosts)).replace('%', "%%");
        Ok(Some(format!("ProxyCommand={inner} -W %h:%p {jump}")))
    }

    /// The shell command that opens the remote attack box. `known_hosts` keeps the
    /// launcher's host keys away from the player's own ~/.ssh.
    pub fn attack_shell_command(&self, known_hosts: &Path) -> Result<String> {
        self.check_endpoint()?;
        let proxy = self.proxy_option(known_hosts)?.map(|p| format!("-o {} ", sh_quote(&p))).unwrap_or_default();
        Ok(format!(
            "ssh -t -i {} -p {} {} {proxy}{}@{} sudo docker exec -it attacker bash",
            sh_quote(&self.identity.to_string_lossy()),
            self.port,
            shell_options(false, known_hosts),
            self.user,
            self.host,
        ))
    }

    /// Runs `command` on the lab host, non-interactively (no password prompt, short
    /// connect timeout). `known_hosts` should be per deployment: a new VM on a reused
    /// address has a new host key.
    pub async fn exec(&self, known_hosts: &Path, command: &str) -> Result<String> {
        let args = self.exec_args(known_hosts, command)?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run("ssh", &refs, None).await
    }

    /// `exec` that gives up after `timeout` (ssh is killed): for short probes that must never
    /// hang their caller, whatever the host or the network does.
    pub async fn exec_timed(&self, known_hosts: &Path, command: &str, timeout: std::time::Duration) -> Result<String> {
        let args = self.exec_args(known_hosts, command)?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::exec::run_env_timed("ssh", &refs, None, &[], timeout).await
    }

    /// The `ssh` arguments `exec` runs (no shell in between).
    fn exec_args(&self, known_hosts: &Path, command: &str) -> Result<Vec<String>> {
        self.check_endpoint()?;
        let mut args: Vec<String> = vec!["-i".into(), self.identity.to_string_lossy().to_string(), "-p".into(), self.port.to_string()];
        for o in options(true, &known_hosts.to_string_lossy()).into_iter().chain(self.proxy_option(known_hosts)?) {
            args.extend(["-o".into(), o]);
        }
        args.extend([format!("{}@{}", self.user, self.host), command.to_string()]);
        Ok(args)
    }
}

pub fn known_hosts(app: &AppHandle) -> Result<PathBuf> {
    Ok(ssh_dir(app)?.join("known_hosts"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vagrant_ssh_config() {
        let out = "Host labhost\n  HostName 192.168.1.50\n  User vagrant\n  Port 22\n  UserKnownHostsFile /dev/null\n  IdentityFile \"/Users/a b/lab/.vagrant/machines/labhost/vmware_esxi/private_key\"\n  IdentitiesOnly yes\n";
        let t = parse_ssh_config(out).unwrap();
        assert_eq!(t.host, "192.168.1.50");
        assert_eq!(t.user, "vagrant");
        assert_eq!(t.identity, PathBuf::from("/Users/a b/lab/.vagrant/machines/labhost/vmware_esxi/private_key"));
    }

    #[test]
    fn command_quotes_paths_and_rejects_odd_hosts() {
        let t = Target { host: "10.0.0.5".into(), port: 22, user: "debian".into(), identity: "/Users/a b/key".into(), jump: None };
        let cmd = t.attack_shell_command(Path::new("/x/known_hosts")).unwrap();
        assert!(cmd.contains("-i '/Users/a b/key'"));
        assert!(cmd.ends_with("debian@10.0.0.5 sudo docker exec -it attacker bash"));
        let bad = Target { host: "10.0.0.5;rm".into(), ..t };
        assert!(bad.attack_shell_command(Path::new("/x")).is_err());
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn attack_shell_command_is_unchanged() {
        let t = Target::direct("10.0.0.5", "debian", "/k/id".into());
        assert_eq!(
            t.attack_shell_command(Path::new("/x/kh")).unwrap(),
            "ssh -t -i '/k/id' -p 22 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/x/kh -o LogLevel=ERROR debian@10.0.0.5 sudo docker exec -it attacker bash"
        );
    }

    #[test]
    fn jump_hosts_become_a_proxy_command() {
        let t = Target { jump: Some("root@pve.lan".into()), ..Target::direct("10.10.0.5", "isoloom", "/k/id".into()) };
        assert_eq!(
            t.proxy_option(Path::new("/x/kh")).unwrap().unwrap(),
            "ProxyCommand=ssh -i '/k/id' -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/x/kh -o LogLevel=ERROR -W %h:%p root@pve.lan"
        );
        let cmd = t.attack_shell_command(Path::new("/x/kh")).unwrap();
        assert!(cmd.contains("-o 'ProxyCommand=ssh -i '\\''/k/id'\\''"), "{cmd}");
        for bad in ["nobody", "root@pve;rm", "-o@x"] {
            let t = Target { jump: Some(bad.into()), ..Target::direct("10.10.0.5", "isoloom", "/k/id".into()) };
            assert!(t.proxy_option(Path::new("/x")).is_err(), "{bad}");
        }
        assert_eq!(Target::direct("h", "u", "/k".into()).proxy_option(Path::new("/x")).unwrap(), None);
    }

    #[test]
    fn options_add_batch_mode_only_when_asked() {
        assert_eq!(options(false, "/kh"), ["StrictHostKeyChecking=accept-new", "UserKnownHostsFile=/kh", "LogLevel=ERROR"]);
        assert_eq!(options(true, "/kh")[..2], ["BatchMode=yes", "ConnectTimeout=10"]);
        // A host that stops answering mid-command ends it within 45 s, not hours later.
        assert_eq!(options(true, "/kh")[2..4], ["ServerAliveInterval=15", "ServerAliveCountMax=3"]);
    }

    #[test]
    fn exec_runs_batch_mode_with_the_key_and_port() {
        let t = Target { port: 2222, ..Target::direct("10.0.0.5", "debian", "/k/id".into()) };
        let args = t.exec_args(Path::new("/x/kh"), "uptime").unwrap();
        assert_eq!(args[..4], ["-i", "/k/id", "-p", "2222"]);
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert!(args.contains(&"UserKnownHostsFile=/x/kh".to_string()));
        assert_eq!(args[args.len() - 2..], ["debian@10.0.0.5", "uptime"]);
        // Through a jump host: one more -o, the proxy command.
        let jumped = Target { jump: Some("root@pve.lan".into()), ..Target::direct("10.10.0.5", "isoloom", "/k/id".into()) };
        assert!(jumped.exec_args(Path::new("/x/kh"), "true").unwrap().iter().any(|a| a.starts_with("ProxyCommand=ssh -i ")));
    }

    #[tokio::test]
    async fn exec_refuses_odd_hosts_before_running_ssh() {
        let bad = Target::direct("-oProxyCommand=x", "debian", "/k".into());
        assert!(bad.exec(Path::new("/x/kh"), "true").await.is_err());
        let bad_user = Target::direct("10.0.0.5", "a b", "/k".into());
        assert!(bad_user.exec(Path::new("/x/kh"), "true").await.is_err());
        let bad_jump = Target { jump: Some("nobody".into()), ..Target::direct("10.0.0.5", "debian", "/k".into()) };
        assert!(bad_jump.exec(Path::new("/x/kh"), "true").await.is_err());
    }

    #[test]
    fn ssh_config_needs_host_user_and_key() {
        assert_eq!(parse_ssh_config("Host x\n  User vagrant\n  IdentityFile /k\n"), None);
        let t = parse_ssh_config("  HostName h\n  User u\n  Port nope\n  IdentityFile /a\n  IdentityFile /b\n").unwrap();
        assert_eq!((t.port, t.identity), (22, PathBuf::from("/a")));
        let t = parse_ssh_config("HostName h\nUser u\nPort 2200\nIdentityFile /a\n").unwrap();
        assert_eq!(t.port, 2200);
    }

    #[test]
    fn safe_tokens_are_plain_hosts_and_users() {
        assert!(safe_token("192.168.1.5") && safe_token("fd00::1") && safe_token("pve_lan-1"));
        assert!(!safe_token("") && !safe_token("-x") && !safe_token("a b") && !safe_token(&"a".repeat(254)));
    }

    #[test]
    fn known_hosts_paths_with_spaces_stay_one_file() {
        let kh = "/Users/a/Library/Application Support/org.cyberctf.desktop/ssh/known_hosts";
        assert_eq!(options(false, kh)[1], r"UserKnownHostsFile=/Users/a/Library/Application\ Support/org.cyberctf.desktop/ssh/known_hosts");
        assert_eq!(ssh_config_path(r"C:\Users\x y\50%\kh"), r"C:\\Users\\x\ y\\50%%\\kh");
        let t = Target::direct("10.0.0.5", "debian", "/k/id".into());
        assert!(t.attack_shell_command(Path::new(kh)).unwrap().contains(r"-o 'UserKnownHostsFile=/Users/a/Library/Application\ Support/"));
    }
}

#[cfg(test)]
mod proptests;
