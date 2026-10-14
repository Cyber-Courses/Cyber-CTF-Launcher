//! After apply the VM exists, but cloud-init is still installing Docker and starting the lab:
//! waiting on the bootstrap's report over SSH, so "running" means the lab is up.

use std::path::Path;
use std::time::{Duration, Instant};

use super::state::{output, ssh_target};
use crate::error::{Error, Result};
use crate::runtime::ssh;

/// How long the lab host may take to install Docker and start the lab after boot.
const READY_TIMEOUT: Duration = Duration::from_secs(40 * 60);
/// Give up on confirming (not on the lab) when SSH never answers this long.
const SSH_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_secs(10);
/// One status read (a `cat` and a `tail`) never takes longer than this; past it the read is
/// abandoned and retried, so the deadlines above hold even when SSH hangs.
const EXEC_TIMEOUT: Duration = Duration::from_secs(60);

/// The bootstrap's report, read from the lab host.
#[derive(Debug, PartialEq)]
enum Progress {
    Running(String),
    Ready,
    Failed(String),
}

/// First line of `cat <ready_file>`: "running: <step>", "ready" or "failed: <step>".
fn parse_progress(first_line: &str) -> Progress {
    let line = first_line.trim();
    if line == "ready" {
        Progress::Ready
    } else if let Some(step) = line.strip_prefix("failed:") {
        Progress::Failed(step.trim().to_string())
    } else {
        Progress::Running(line.strip_prefix("running:").unwrap_or("booting").trim().to_string())
    }
}

/// Reads one answer of the lab host (its status line, then its log's tail): the end of the wait
/// (ready, or failed with the log), or None while it is still going (a new step is logged).
fn report(out: &str, last_step: &mut String, log: &mut impl FnMut(String)) -> Option<Result<()>> {
    let mut lines = out.lines();
    match parse_progress(lines.next().unwrap_or_default()) {
        Progress::Ready => {
            log("Lab host ready.".into());
            Some(Ok(()))
        }
        Progress::Failed(step) => {
            let tail: Vec<&str> = lines.filter(|l| !l.trim().is_empty()).collect();
            Some(Err(Error::CommandFailed { command: format!("lab host: {step}"), stderr: tail.join("\n") }))
        }
        Progress::Running(step) => {
            if step != *last_step {
                log(format!("Lab host: {step}…"));
                *last_step = step;
            }
            None
        }
    }
}

/// Waits for the bootstrap's `ready_file` over SSH; a failed bootstrap fails the launch with
/// its log. Modules without a `ready_file` output (older labs) are not waited on.
pub(super) async fn wait_ready(state: &Path, log: &mut impl FnMut(String)) -> Result<()> {
    let Some(ready_file) = output(state, crate::runtime::lab::outputs::READY_FILE) else { return Ok(()) };
    let Some(target) = ssh::launcher_key().and_then(|identity| ssh_target(state, identity)) else {
        log("Can't reach the lab host over SSH to confirm it started; check it from the lab page.".into());
        return Ok(());
    };
    let known_hosts = super::known_hosts(state);
    let command = format!("cat {} 2>/dev/null || true; echo; tail -n 25 /var/log/cyberctf-lab.log 2>/dev/null || true", ssh::sh_quote(&ready_file));

    log("Waiting for the lab host to install Docker and start the lab…".into());
    let started = Instant::now();
    let mut reached = false;
    let mut last_step = String::new();
    loop {
        match target.exec_timed(&known_hosts, &command, EXEC_TIMEOUT).await {
            Ok(out) => {
                reached = true;
                if let Some(done) = report(&out, &mut last_step, log) {
                    return done;
                }
            }
            Err(_) if !reached && started.elapsed() >= SSH_TIMEOUT => {
                log("The lab host doesn't answer over SSH, so the launcher can't confirm the lab started. It may still be starting; check it from the lab page.".into());
                return Ok(());
            }
            // Booting (SSH not up yet) or a dropped connection: try again.
            Err(_) => {}
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err(Error::Invalid(format!(
                "the lab host didn't finish starting the lab within {} minutes (last step: {})",
                READY_TIMEOUT.as_secs() / 60,
                if last_step.is_empty() { "booting" } else { &last_step }
            )));
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::terraform::tests::temp_state;

    #[test]
    fn parses_bootstrap_progress() {
        assert_eq!(parse_progress("ready\n"), Progress::Ready);
        assert_eq!(parse_progress("running: downloading the lab"), Progress::Running("downloading the lab".into()));
        assert_eq!(parse_progress("failed: starting the lab"), Progress::Failed("starting the lab".into()));
        // No status file yet: cloud-init hasn't reached the bootstrap.
        assert_eq!(parse_progress(""), Progress::Running("booting".into()));
    }

    #[test]
    fn each_answer_logs_a_new_step_once_and_ends_on_ready_or_failed() {
        let mut lines = Vec::new();
        let mut last = String::new();
        assert!(report("running: pulling images\n", &mut last, &mut |l| lines.push(l)).is_none());
        assert!(report("running: pulling images\n", &mut last, &mut |l| lines.push(l)).is_none());
        assert!(report("\n", &mut last, &mut |l| lines.push(l)).is_none());
        assert_eq!(lines, ["Lab host: pulling images…", "Lab host: booting…"]);
        assert!(matches!(report("ready\n", &mut last, &mut |l| lines.push(l)), Some(Ok(()))));
        assert_eq!(lines.last().unwrap(), "Lab host ready.");
        match report("failed: starting the lab\n\nerror: port taken\n  \nexit 1\n", &mut last, &mut |l| lines.push(l)) {
            Some(Err(Error::CommandFailed { command, stderr })) => {
                assert_eq!(command, "lab host: starting the lab");
                assert_eq!(stderr, "error: port taken\nexit 1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_ready_file_without_an_address_is_not_waited_on() {
        let dir = temp_state();
        std::fs::write(dir.join("terraform.tfstate"), r#"{"outputs":{"ready_file":{"value":"/var/lib/cyberctf/ready"}}}"#).unwrap();
        let mut lines = Vec::new();
        wait_ready(&dir, &mut |l| lines.push(l)).await.unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Can't reach the lab host over SSH"), "{lines:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn no_ready_file_output_means_no_wait() {
        let dir = temp_state();
        std::fs::write(dir.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"10.0.0.9"}}}"#).unwrap();
        let mut lines = Vec::new();
        wait_ready(&dir, &mut |l| lines.push(l)).await.unwrap();
        assert!(lines.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod proptests;
