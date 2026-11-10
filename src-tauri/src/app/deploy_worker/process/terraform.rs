//! The Terraform runs under a deploy worker, found and interrupted: Terraform's own clean stop
//! (it finishes what it is creating and records it in its state), which a stop sends before it
//! ends the worker. Through `pgrep` and signals on Unix, a process snapshot and a console Ctrl+C on
//! Windows.

/// Sends SIGINT to the Terraform runs in process group `group` (the host binary, or the `docker
/// run` of its container, which hands the signal on): Terraform's own clean stop. Whether one
/// was found.
#[cfg(unix)]
pub(super) fn interrupt_terraform(group: u32) -> bool {
    let pgrep = |args: &[&str]| pgrep_with("pgrep", args);
    // The worker's group, and those of the tools it streams: each leads its own (exec::stream).
    let mut groups = vec![group];
    groups.extend(pgrep(&["-P", &group.to_string()]));
    let groups = groups.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut found = pgrep(&["-g", &groups, "-x", "terraform"]);
    found.extend(pgrep(&["-g", &groups, "-f", "hashicorp/terraform"]));
    for pid in &found {
        if let Err(e) = crate::platform::sys::signal_process(*pid, libc::SIGINT) {
            log::warn!("couldn't interrupt Terraform ({pid}) under deploy worker {group}: {e}");
        }
    }
    !found.is_empty()
}

/// The process ids `program` (`pgrep`) prints for `args`. One that can't run, or that fails (not
/// "nothing matched", its exit 1), is logged: a stop then finds no Terraform to interrupt and ends
/// the worker as any other, mid-apply, which is what interrupting it first is there to avoid.
#[cfg(unix)]
fn pgrep_with(program: &str, args: &[&str]) -> Vec<u32> {
    match std::process::Command::new(program).args(args).output() {
        Ok(o) if o.status.success() || o.status.code() == Some(1) => pids(&String::from_utf8_lossy(&o.stdout)),
        Ok(o) => {
            log::warn!(
                "`{program} {}` failed ({}), so a stop finds no Terraform to interrupt: {}",
                args.join(" "),
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            );
            Vec::new()
        }
        Err(e) => {
            log::warn!("`{program}` can't run, so a stop finds no Terraform to interrupt: {e}");
            Vec::new()
        }
    }
}

/// Windows: a Ctrl+C on the console of each Terraform run under the worker `root` (the host
/// binary, or the `docker run` of its container, which hands it on as SIGINT). Whether one was
/// interrupted.
#[cfg(windows)]
pub(super) fn interrupt_terraform(root: u32) -> bool {
    let found = terraform_runs_under(root);
    found.into_iter().filter(|&pid| crate::platform::console::interrupt(pid)).count() > 0
}

/// The Terraform runs among `root`'s descendants, from a snapshot of all processes.
#[cfg(windows)]
fn terraform_runs_under(root: u32) -> Vec<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
    let procs: Vec<Proc> = sys
        .processes()
        .iter()
        .map(|(pid, p)| Proc {
            pid: pid.as_u32(),
            parent: p.parent().map(sysinfo::Pid::as_u32),
            name: p.name().to_string_lossy().to_lowercase(),
            cmdline: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "),
        })
        .collect();
    terraform_pids(&procs, root)
}

#[cfg(any(windows, test))]
struct Proc {
    pid: u32,
    parent: Option<u32>,
    name: String,
    cmdline: String,
}

/// The Terraform runs below `root` in `procs`: `terraform.exe`, or a `docker run` of the
/// hashicorp/terraform image. Not their own children (providers share their console, so the one
/// Ctrl+C reaches them too).
#[cfg(any(windows, test))]
fn terraform_pids(procs: &[Proc], root: u32) -> Vec<u32> {
    let is_terraform =
        |p: &Proc| p.name == "terraform.exe" || p.name == "terraform" || (p.name.starts_with("docker") && p.cmdline.contains("hashicorp/terraform"));
    let parent_of = |pid: u32| procs.iter().find(|p| p.pid == pid).and_then(|p| p.parent);
    // Under `root`, and not under another Terraform run. Depth-capped: a pid can be reused, so
    // the parent chain read from a snapshot may loop.
    let under_root = |p: &Proc| {
        let mut cur = p.parent;
        for _ in 0..32 {
            match cur {
                Some(pid) if pid == root => return true,
                Some(pid) if procs.iter().any(|q| q.pid == pid && is_terraform(q)) => return false,
                Some(pid) => cur = parent_of(pid),
                None => return false,
            }
        }
        false
    };
    procs.iter().filter(|p| p.pid != root && is_terraform(p) && under_root(p)).map(|p| p.pid).collect()
}

/// The process ids `pgrep` printed, one per line.
#[cfg(any(unix, test))]
pub(super) fn pids(out: &str) -> Vec<u32> {
    out.lines().filter_map(|l| l.trim().parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn the_terraform_runs_found_are_the_workers_and_not_their_providers() {
        let p = |pid, parent, name: &str, cmdline: &str| Proc { pid, parent: Some(parent), name: name.into(), cmdline: cmdline.into() };
        let procs = [
            p(100, 1, "cyberctf-desktop.exe", "cyberctf-desktop.exe deploy --job a.json"),
            p(110, 100, "terraform.exe", "terraform apply -auto-approve"),
            p(111, 110, "terraform-provider-aws_v5.exe", ""),
            // Nested terraform (a module calling out): its parent run's Ctrl+C reaches it.
            p(112, 110, "terraform.exe", "terraform version"),
            p(120, 100, "docker.exe", "docker run --rm hashicorp/terraform:1.9.8 apply"),
            p(130, 100, "docker.exe", "docker compose up"),
            // Another worker's run, and a pid cycle from reuse: neither is this worker's.
            p(200, 1, "cyberctf-desktop.exe", "cyberctf-desktop.exe deploy --job b.json"),
            p(210, 200, "terraform.exe", "terraform apply"),
            p(300, 301, "terraform.exe", ""),
            p(301, 300, "cmd.exe", ""),
        ];
        let mut found = terraform_pids(&procs, 100);
        found.sort_unstable();
        assert_eq!(found, [110, 120]);
        assert_eq!(terraform_pids(&procs, 999), [] as [u32; 0]);
    }

    #[test]
    fn pgrep_output_reads_as_pids() {
        assert_eq!(pids("123\n 456\n\nnope\n"), [123, 456]);
        assert_eq!(pids(""), [] as [u32; 0]);
    }

    /// Waits for a stand-in Terraform to print `ready` on `out`: it has started, under its own
    /// command line.
    #[cfg(unix)]
    fn ready(out: std::process::ChildStdout) {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::BufReader::new(out).read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
    }

    #[cfg(unix)]
    #[test]
    fn a_stop_interrupts_terraform_rather_than_killing_it() {
        // Regression: Stop during a cloud `terraform apply` SIGTERMed the worker's whole group,
        // then SIGKILLed it, so Terraform never recorded what it was creating and the destroy
        // that followed left those resources running. Terraform alone gets SIGINT now (its clean
        // stop). Terraform here is a stand-in for its container run (`… hashicorp/terraform …`),
        // in a group of its own like a worker's.
        use std::os::unix::process::CommandExt;
        let mut run = std::process::Command::new("sh")
            .args(["-c", "echo ready; sleep 30 & wait", "docker", "run", "hashicorp/terraform:1.9.8"])
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let group = run.id();
        // A group with no Terraform in it: nothing is sent.
        let mut other = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        assert!(!interrupt_terraform(other.id()));
        // Once it says so, the stand-in runs under its own command line, for `pgrep` to find:
        // waited for, however long a loaded machine takes, not guessed with a few short sleeps.
        ready(run.stdout.take().unwrap());
        assert!(interrupt_terraform(group), "terraform found in its group");
        let status = run.wait().unwrap();
        // Ended by the interrupt (its sleep alone would take 30 s), however the shell reports it.
        assert!(!status.success(), "{status:?}");
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_pgrep_that_cant_run_or_fails_is_logged() {
        // Regression: a pgrep that couldn't run read as "no Terraform here", and the stop then
        // ended the worker mid-apply with nothing in the log.
        crate::testing::logs::take_warnings();
        assert_eq!(pgrep_with("cyberctf-no-such-pgrep", &["-x", "terraform"]), [] as [u32; 0]);
        let logged = crate::testing::logs::take_warnings();
        assert!(logged.iter().any(|w| w.starts_with("`cyberctf-no-such-pgrep` can't run, so a stop finds no Terraform")), "{logged:?}");
        // Nothing matched (exit 1) is no failure; a failure (exit 2) is.
        assert_eq!(pgrep_with("sh", &["-c", "exit 1"]), [] as [u32; 0]);
        assert_eq!(crate::testing::logs::take_warnings(), [] as [String; 0]);
        assert_eq!(pgrep_with("sh", &["-c", "echo 42; echo 43"]), [42, 43]);
        assert_eq!(pgrep_with("sh", &["-c", "echo bad option >&2; exit 2"]), [] as [u32; 0]);
        let logged = crate::testing::logs::take_warnings();
        assert!(logged.iter().any(|w| w.starts_with("`sh -c echo bad option >&2; exit 2` failed") && w.ends_with("bad option")), "{logged:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_stop_reaches_terraform_in_a_group_of_its_own() {
        // A worker streams each tool in a process group of its own (so a cancel ends the tool's
        // whole tree), out of the worker's group: Terraform's interrupt still finds it there.
        // The stand-in worker starts it in a group of its own (perl's setpgrp, with SIGINT back
        // to its default: `&` ignores it); the image name comes from the environment, so only
        // the stand-in Terraform's command line names it.
        use std::os::unix::process::CommandExt;
        let run = r#"perl -e '$SIG{INT} = "DEFAULT"; setpgrp(0, 0); exec @ARGV' sh -c 'echo ready; sleep 30 & wait' docker run "$IMAGE" & wait"#;
        let mut worker = std::process::Command::new("sh")
            .args(["-c", run])
            .env("IMAGE", "hashicorp/terraform:1.9.8")
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // The stand-in Terraform says when it runs, in its own group under the worker.
        ready(worker.stdout.take().unwrap());
        assert!(interrupt_terraform(worker.id()), "terraform found in its own group under the worker");
        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(move || worker.wait());
        while !waiter.is_finished() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(waiter.is_finished(), "the interrupted run ended, and the worker with it");
    }
}
