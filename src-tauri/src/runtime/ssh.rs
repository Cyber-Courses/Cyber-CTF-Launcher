//! Reaching a remote lab host over SSH, for "Open shell" on labs that run on a server
//! host or in the cloud: the attack box runs next to the lab there, so the shell is
//! `ssh <lab host> sudo docker exec -it attacker bash`.
//!
//! The launcher has its own key (`<app data>/ssh/id_ed25519`), made once with the
//! system's `ssh-keygen` and installed on lab hosts by cloud-init (`ssh_public_key`).
//! Vagrant targets (ESXi) use Vagrant's own key, read from `vagrant ssh-config`.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::exec::run;

/// The `known_hosts` every attack box shell once shared. Its keys of lab hosts long gone refused
/// the shell to any new lab host at a recycled address: removed (nothing writes it any more).
const SHARED_KNOWN_HOSTS: &str = "known_hosts";

pub mod known_hosts;

fn ssh_dir(dirs: &AppDirs) -> Result<PathBuf> {
    let dir = dirs.data()?.join("ssh");
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::remove_file(dir.join(SHARED_KNOWN_HOSTS));
    Ok(dir)
}

/// The launcher's private key, created on first use. Returns (key path, public key line).
pub async fn ensure_key(dirs: &AppDirs) -> Result<(PathBuf, String)> {
    let key = ssh_dir(dirs)?.join("id_ed25519");
    if !key.is_file() {
        let path = key.to_string_lossy().to_string();
        run("ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-C", "cyberctf-launcher", "-f", &path], None).await?;
    }
    let public = std::fs::read_to_string(key.with_extension("pub"))?.trim().to_string();
    Ok((key, public))
}

/// Where to SSH: what `ssh` needs, nothing more.
#[derive(Debug, PartialEq)]
pub struct Target {
    /// The address to connect to.
    pub host: String,
    /// Its SSH port.
    pub port: u16,
    /// The user to sign in as.
    pub user: String,
    /// The private key to sign in with.
    pub identity: PathBuf,
    /// `user@host` to go through (with the same key) when `host` isn't routable from here: a
    /// lab VM on a Proxmox node's host-internal bridge, reached via the node.
    pub jump: Option<String>,
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

/// The `-o` values every launcher SSH uses: only the host keys the learner confirmed, in the
/// launcher's own `known_hosts` ([`known_hosts`], never the player's ~/.ssh), are accepted; a host
/// with none or another is refused, never pinned silently. Quiet. `known_hosts` is the plain
/// path; it is escaped for ssh here.
pub fn options(known_hosts: &str) -> Vec<String> {
    vec!["StrictHostKeyChecking=yes".into(), format!("UserKnownHostsFile={}", ssh_config_path(known_hosts)), "LogLevel=ERROR".into()]
}

/// [`options`] for an SSH that runs unattended: no prompts and a short connect timeout, and
/// keepalives so a host that stops answering mid-command (a VM that froze, a dropped network)
/// ends the command in under a minute instead of hanging it for hours.
pub fn batch_options(known_hosts: &str) -> Vec<String> {
    let mut opts: Vec<String> = vec!["BatchMode=yes".into(), "ConnectTimeout=10".into(), "ServerAliveInterval=15".into(), "ServerAliveCountMax=3".into()];
    opts.extend(options(known_hosts));
    opts
}

/// How the ssh on this computer runs a `ProxyCommand` or an askpass helper: through `sh` on unix,
/// as a plain Windows command line (or a `.cmd`) on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandLine {
    /// `sh -c`'s rules.
    Sh,
    /// The C runtime's rules, as Windows' OpenSSH starts a command.
    Windows,
}

impl CommandLine {
    /// This computer's.
    pub(crate) const HERE: CommandLine = if cfg!(windows) { CommandLine::Windows } else { CommandLine::Sh };
}

/// Whether `s` is a word no command line needs to quote.
fn plain_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "=.,_/:@+-".contains(c))
}

/// A word of the `ProxyCommand` ssh runs: ssh hands it to `sh -c` on unix, but Windows' OpenSSH
/// starts it as a plain Windows command line, where a single quote is a plain character (the
/// key's path then arrived wrapped in quotes and ssh couldn't open it). There a word is quoted
/// the way the C runtime splits a command line back ([`windows_quote`]).
fn proxy_word(s: &str, line: CommandLine) -> String {
    match (line, plain_word(s)) {
        (_, true) => s.to_string(),
        (CommandLine::Windows, false) => windows_quote(s),
        (CommandLine::Sh, false) => sh_quote(s),
    }
}

/// `s` double-quoted for a Windows command line (the C runtime's rules, as `CommandLineToArgvW`
/// reads them): backslashes are literal except before a `"`, so those before an inner `"` (then
/// escaped) and before the closing one are doubled.
fn windows_quote(s: &str) -> String {
    let mut out = String::from("\"");
    // The backslashes just written, doubled if a `"` follows them.
    let mut run = 0;
    for c in s.chars() {
        if c == '"' {
            out.extend(std::iter::repeat_n('\\', run + 1));
        }
        run = if c == '\\' { run + 1 } else { 0 };
        out.push(c);
    }
    out.extend(std::iter::repeat_n('\\', run));
    out.push('"');
    out
}

/// `batch_options` as `-o …` words for a command line, each quoted by `quote`.
fn quoted_options(known_hosts: &Path, quote: impl Fn(&str) -> String) -> String {
    batch_options(&known_hosts.to_string_lossy()).iter().map(|o| format!("-o {}", quote(o))).collect::<Vec<_>>().join(" ")
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

    /// `-o ProxyCommand=…` through the jump host, with the same key and `known_hosts`, quoted for
    /// this OS's ssh.
    fn proxy_option(&self, known_hosts: &Path) -> Result<Option<String>> {
        self.proxy_command(known_hosts, CommandLine::HERE)
    }

    /// [`Target::proxy_option`] for an ssh that runs it as `line`.
    fn proxy_command(&self, known_hosts: &Path, line: CommandLine) -> Result<Option<String>> {
        let Some(jump) = &self.jump else { return Ok(None) };
        let (user, host) = jump.split_once('@').ok_or_else(|| Error::Invalid("unexpected jump host".into()))?;
        if !safe_token(user) || !safe_token(host) {
            return Err(Error::Invalid("unexpected jump host".into()));
        }
        // ssh expands `%` tokens in a ProxyCommand before its shell runs it: only `%h:%p` is ours.
        let quote = |s: &str| proxy_word(s, line);
        let inner = format!("ssh -i {} {}", quote(&self.identity.to_string_lossy()), quoted_options(known_hosts, quote)).replace('%', "%%");
        Ok(Some(format!("ProxyCommand={inner} -W %h:%p {jump}")))
    }

    /// `exec` that gives up after `timeout` (ssh is killed): for short probes that must never
    /// hang their caller, whatever the host or the network does.
    pub async fn exec_timed(&self, known_hosts: &Path, command: &str, timeout: std::time::Duration) -> Result<String> {
        let args = self.exec_args(known_hosts, command)?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::platform::exec::run_env_timed("ssh", &refs, None, &[], timeout).await.map_err(|e| known_hosts::refused_key(&self.host, e))
    }

    /// The node login `login` (`user@host`, as a jump names it) on port 22, signed in with
    /// `identity`; refused when it isn't one.
    pub fn login(login: &str, identity: PathBuf) -> Result<Self> {
        let (user, host) = login.split_once('@').ok_or_else(|| Error::Invalid("unexpected jump host".into()))?;
        let target = Target::direct(host, user, identity);
        target.check_endpoint()?;
        Ok(target)
    }

    /// The `ssh` arguments of a SOCKS5 proxy on `127.0.0.1:port` through this host, signing in
    /// to nothing else (`-N`): unattended, its host key checked against `known_hosts`, and ending
    /// when the port can't be forwarded (taken meanwhile) rather than running on as no proxy.
    pub fn socks_args(&self, known_hosts: &Path, port: u16) -> Result<Vec<String>> {
        self.check_endpoint()?;
        let mut args: Vec<String> = vec!["-N".into(), "-D".into(), format!("127.0.0.1:{port}")];
        args.extend(["-i".into(), self.identity.to_string_lossy().to_string(), "-p".into(), self.port.to_string()]);
        for o in batch_options(&known_hosts.to_string_lossy()).into_iter().chain(["ExitOnForwardFailure=yes".to_owned()]) {
            args.extend(["-o".into(), o]);
        }
        args.push(format!("{}@{}", self.user, self.host));
        Ok(args)
    }

    /// The `ssh` arguments `exec` runs (no shell in between).
    fn exec_args(&self, known_hosts: &Path, command: &str) -> Result<Vec<String>> {
        self.check_endpoint()?;
        let mut args: Vec<String> = vec!["-i".into(), self.identity.to_string_lossy().to_string(), "-p".into(), self.port.to_string()];
        for o in batch_options(&known_hosts.to_string_lossy()).into_iter().chain(self.proxy_option(known_hosts)?) {
            args.extend(["-o".into(), o]);
        }
        args.extend([format!("{}@{}", self.user, self.host), command.to_string()]);
        Ok(args)
    }
}

/// The host keys of the server nodes the launcher reaches with its key (a Proxmox node: the jump
/// host to an internal lab bridge, its key setup), next to that key, as the learner confirmed
/// them ([`known_hosts`]). A lab host's keys are never kept here: the Isothrum runner keeps each
/// run's own, in the run's folder.
pub fn node_known_hosts(identity: &Path) -> PathBuf {
    identity.with_file_name("nodes_known_hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_with_odd_hosts_are_rejected() {
        let t = Target { host: "10.0.0.5".into(), port: 22, user: "debian".into(), identity: "/Users/a b/key".into(), jump: None };
        assert!(t.exec_args(Path::new("/x/kh"), "true").is_ok());
        let bad = Target { host: "10.0.0.5;rm".into(), ..t };
        assert!(bad.exec_args(Path::new("/x"), "true").is_err());
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn jump_hosts_become_a_proxy_command() {
        let t = Target { jump: Some("root@pve.lan".into()), ..Target::direct("10.10.0.5", "isoloom", "/k/id".into()) };
        assert_eq!(
            t.proxy_option(Path::new("/x/kh")).unwrap().unwrap(),
            "ProxyCommand=ssh -i /k/id -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/x/kh -o LogLevel=ERROR -W %h:%p root@pve.lan"
        );
        for bad in ["nobody", "root@pve;rm", "-o@x"] {
            let t = Target { jump: Some(bad.into()), ..Target::direct("10.10.0.5", "isoloom", "/k/id".into()) };
            assert!(t.proxy_option(Path::new("/x")).is_err(), "{bad}");
        }
        assert_eq!(Target::direct("h", "u", "/k".into()).proxy_option(Path::new("/x")).unwrap(), None);
    }

    #[test]
    fn a_windows_proxy_command_double_quotes_its_paths() {
        // Regression: Windows' ssh runs a ProxyCommand with no POSIX shell, so the single-quoted
        // key path reached the jump host's ssh with its quotes, and it failed.
        let key = PathBuf::from(r"C:\Users\Frédéric O'Brien\AppData\Roaming\org.cyberctf.desktop\ssh\id_ed25519");
        let t = Target { jump: Some("root@pve.lan".into()), ..Target::direct("10.10.0.5", "isoloom", key.clone()) };
        let kh = Path::new(r"C:\Users\Frédéric O'Brien\kh");
        let proxy = t.proxy_command(kh, CommandLine::Windows).unwrap().unwrap();
        assert!(proxy.starts_with(&format!("ProxyCommand=ssh -i \"{}\" -o BatchMode=yes ", key.display())), "{proxy}");
        assert!(proxy.contains(r#"-o "UserKnownHostsFile=C:\\Users\\Frédéric\ O\'Brien\\kh""#), "{proxy}");
        assert!(proxy.ends_with(" -W %h:%p root@pve.lan"), "{proxy}");
        // Plain words stay bare, and unix keeps POSIX quoting.
        assert!(proxy.contains(" -o LogLevel=ERROR "));
        assert!(t.proxy_command(kh, CommandLine::Sh).unwrap().unwrap().contains(r"-i 'C:\Users\Frédéric O'\''Brien\"));
        assert_eq!(proxy_word("a b", CommandLine::Windows), "\"a b\"");
        assert_eq!(proxy_word("a b", CommandLine::Sh), "'a b'");
        assert_eq!(proxy_word("BatchMode=yes", CommandLine::Windows), "BatchMode=yes");
    }

    #[test]
    fn options_add_batch_mode_only_when_asked() {
        assert_eq!(options("/kh"), ["StrictHostKeyChecking=yes", "UserKnownHostsFile=/kh", "LogLevel=ERROR"]);
        assert_eq!(batch_options("/kh")[..2], ["BatchMode=yes", "ConnectTimeout=10"]);
        // A host that stops answering mid-command ends it within 45 s, not hours later.
        assert_eq!(batch_options("/kh")[2..4], ["ServerAliveInterval=15", "ServerAliveCountMax=3"]);
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

    #[test]
    fn safe_tokens_are_plain_hosts_and_users() {
        assert!(safe_token("192.168.1.5") && safe_token("fd00::1") && safe_token("pve_lan-1"));
        assert!(!safe_token("") && !safe_token("-x") && !safe_token("a b") && !safe_token(&"a".repeat(254)));
    }

    #[test]
    fn the_shared_known_hosts_is_dropped_and_nodes_keep_their_own() {
        let t = crate::testing::test_app::TestApp::new();
        let shared = t.data().join("ssh").join(SHARED_KNOWN_HOSTS);
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "10.0.0.5 ssh-ed25519 AAAAold\n").unwrap();
        let dir = ssh_dir(&t.dirs()).unwrap();
        assert!(!shared.exists());
        assert_eq!(node_known_hosts(&dir.join("id_ed25519")), dir.join("nodes_known_hosts"));
    }

    #[test]
    fn known_hosts_paths_with_spaces_stay_one_file() {
        let kh = "/Users/a/Library/Application Support/org.cyberctf.desktop/ssh/known_hosts";
        assert_eq!(options(kh)[1], r"UserKnownHostsFile=/Users/a/Library/Application\ Support/org.cyberctf.desktop/ssh/known_hosts");
        assert_eq!(ssh_config_path(r"C:\Users\x y\50%\kh"), r"C:\\Users\\x\ y\\50%%\\kh");
        // The jump-host ProxyCommand carries the same escaped path (see
        // jump_hosts_become_a_proxy_command).
        let t = Target { jump: Some("root@pve.lan".into()), ..Target::direct("10.0.0.5", "debian", "/k/id".into()) };
        assert!(t.proxy_option(Path::new(kh)).unwrap().unwrap().contains(r"UserKnownHostsFile=/Users/a/Library/Application\ Support/"));
    }
}

#[cfg(test)]
mod proptests;
