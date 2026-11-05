//! Who holds a host port, so a port-clash failure can name it and tell the player what to stop.
//!
//! A local VM run forwards the lab's published ports to fixed host ports, and a container run can
//! be asked to keep the lab's own ports, so either can fail because something already listens on
//! one. This looks up what that is, best-effort and quickly: it runs only after a start has
//! already failed, so it never holds a start up for long and never turns a failed lookup into a
//! failed start.

use std::time::Duration;

use crate::error::{Code, Error};
use crate::platform::exec;

/// How long the lookup may take before it is abandoned. It runs on the failure path only, so it
/// stays short: a missing answer is better than a slow one.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(4);

/// `e` with the holder of its taken host port filled in, when it is a port clash whose holder
/// isn't known yet and one can be found. Any other error, and one whose holder is already set, is
/// returned unchanged.
pub(super) async fn with_holder(e: Error) -> Error {
    if let Code::PortInUse { port, holder: None } = e.code()
        && let Some(holder) = tcp_listener(port).await
    {
        return Error::coded(Code::PortInUse { port, holder: Some(holder) });
    }
    e
}

/// A short description of what listens on host TCP `port` ("nginx (pid 4123)"), or None when
/// nothing obvious holds it or the lookup isn't available on this machine.
async fn tcp_listener(port: u16) -> Option<String> {
    #[cfg(windows)]
    {
        windows_listener(port).await
    }
    #[cfg(not(windows))]
    {
        unix_listener(port).await
    }
}

/// macOS and Linux: `lsof` is on both; on a Linux box without it, `ss` is the fallback.
#[cfg(not(windows))]
async fn unix_listener(port: u16) -> Option<String> {
    let spec = format!("-iTCP:{port}");
    if let Ok(text) = exec::run_env_timed("lsof", &["-nP", &spec, "-sTCP:LISTEN", "-FpcL"], None, &[], LOOKUP_TIMEOUT).await
        && let Some(holder) = from_lsof(&text)
    {
        return Some(holder);
    }
    let filter = format!("( sport = :{port} )");
    let text = exec::run_env_timed("ss", &["-H", "-ltnp", &filter], None, &[], LOOKUP_TIMEOUT).await.ok()?;
    from_ss(&text)
}

/// Windows: ask the TCP stack which process listens on the port, then its name.
#[cfg(windows)]
async fn windows_listener(port: u16) -> Option<String> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; $c = Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -First 1; \
         if ($c) {{ $p = Get-Process -Id $c.OwningProcess; if ($p) {{ Write-Output \"$($p.ProcessName) (pid $($p.Id))\" }} }}"
    );
    let text = exec::run_env_timed("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None, &[], LOOKUP_TIMEOUT).await.ok()?;
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(ToOwned::to_owned)
}

/// The first process in `lsof -FpcL` output: its records are field lines, `p<pid>` then `c<command>`.
#[cfg(not(windows))]
fn from_lsof(text: &str) -> Option<String> {
    let mut pid: Option<&str> = None;
    for line in text.lines() {
        let Some(tag) = line.chars().next() else { continue };
        let rest = &line[tag.len_utf8()..];
        match tag {
            'p' => pid = Some(rest),
            'c' => {
                if let Some(pid) = pid {
                    return Some(format_holder(rest, pid));
                }
            }
            _ => {}
        }
    }
    None
}

/// The first listener in `ss -H -ltnp` output: `… users:(("nginx",pid=1234,fd=6))`.
#[cfg(not(windows))]
fn from_ss(text: &str) -> Option<String> {
    let users = text.lines().next()?.split("users:((").nth(1)?;
    let name = users.split('"').nth(1)?;
    let pid = users.split("pid=").nth(1)?.split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())?;
    Some(format_holder(name, pid))
}

/// "nginx (pid 4123)", with Docker's several process names (and `lsof`'s 9-character truncation)
/// shown as the one name the player knows it by.
#[cfg(not(windows))]
fn format_holder(command: &str, pid: &str) -> String {
    format!("{} (pid {pid})", friendly(command.trim()))
}

/// The name the player knows a process by, for the handful that would otherwise read as cryptic.
#[cfg(not(windows))]
fn friendly(command: &str) -> &str {
    match command {
        "com.docke" | "Docker" | "docker" | "dockerd" | "docker-pr" | "docker-proxy" | "vpnkit" | "vpnkit-brid" => "Docker",
        other => other,
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_listener_from_lsof() {
        let out = "p4123\ncnginx\nLroot\nftxt\np5678\ncpython3\n";
        assert_eq!(from_lsof(out).as_deref(), Some("nginx (pid 4123)"));
    }

    #[test]
    fn lsof_names_docker_by_its_player_facing_name() {
        // macOS shows Docker's listener as the truncated `com.docke` (field line `ccom.docke`).
        assert_eq!(from_lsof("p700\nccom.docke\n").as_deref(), Some("Docker (pid 700)"));
        assert_eq!(from_lsof("p701\ncdocker-pr\n").as_deref(), Some("Docker (pid 701)"));
    }

    #[test]
    fn a_c_line_before_any_p_line_is_ignored() {
        assert_eq!(from_lsof("cnginx\np4123\ncnginx\n").as_deref(), Some("nginx (pid 4123)"));
    }

    #[test]
    fn empty_lsof_output_names_nothing() {
        assert_eq!(from_lsof(""), None);
    }

    #[test]
    fn reads_name_and_pid_from_ss() {
        let out = r#"LISTEN 0 4096 0.0.0.0:8080 0.0.0.0:* users:(("nginx",pid=1234,fd=6))"#;
        assert_eq!(from_ss(out).as_deref(), Some("nginx (pid 1234)"));
    }

    #[test]
    fn ss_without_a_process_column_names_nothing() {
        // `ss` without root can't show the process; then there is nothing to name.
        assert_eq!(from_ss("LISTEN 0 4096 0.0.0.0:8080 0.0.0.0:*"), None);
    }
}
