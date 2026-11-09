//! What the real-lab tests share: a lab installed for a test and run through the runner, the
//! test's log, its checks (each failure collected, so the lab is always stopped after), what a
//! lab leaves on Docker, and a probe run through the console the lab page opens.

use std::path::{Path, PathBuf};

use isothrum_runner::ShellCommand;

use crate::error::Result;
use crate::platform::exec::run;
use crate::runtime::{LabStatus, docker, isothrum, launch, lifecycle};

/// One lab installed for a test, run through the Isothrum runner.
pub(crate) struct Run {
    pub(crate) dir: PathBuf,
    pub(crate) id: String,
    /// The runner's home (its runs under `runs/`).
    pub(crate) home: PathBuf,
}

impl Run {
    /// Starts the lab with the launch's values `env`.
    pub(crate) async fn start(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        launch::start_docker_run(&self.home, &self.dir, &self.id, env, log).await
    }

    pub(crate) async fn status(&self) -> Result<LabStatus> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.status().await
    }

    pub(crate) async fn check(&self) -> Result<docker::Check> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.check().await
    }

    /// Parks the lab (its containers stopped, kept), as the lab page's Shut down does.
    pub(crate) async fn park(&self, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.park(crate::runtime::Park::Shutdown, log).await
    }

    /// Starts the parked lab again, as it was.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.resume(log).await
    }

    pub(crate) async fn stop(&self, log: impl FnMut(String)) -> Result<()> {
        lifecycle::stop_docker_run(&self.home, &self.dir, &self.id, log).await
    }
}

/// A fresh copy of the lab, as the launcher installs one: its own folder, nothing generated.
/// Copied in Rust, not with `cp -R`, so the test runs on Windows too (R7).
pub(crate) fn install_copy(src: &Path, id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    crate::runtime::lab::copy_tree(src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    dir
}

pub(crate) fn log(id: &str) -> impl FnMut(String) + use<> {
    let tag = id.to_string();
    move |l: String| println!("[{tag}] {l}")
}

async fn docker_lines(args: &[&str]) -> Vec<String> {
    run("docker", args, None).await.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Everything of the lab Docker still holds: the project's containers (one-off `run` containers
/// and the attack box included: they carry the project label too) and networks.
pub(crate) async fn leftovers(id: &str) -> Vec<String> {
    let label = format!("label=com.docker.compose.project={}", docker::project(id));
    let mut left = Vec::new();
    for (what, args) in [
        ("container", vec!["ps", "-a", "--filter", label.as_str(), "--format", "{{.Names}}"]),
        ("network", vec!["network", "ls", "--filter", label.as_str(), "--format", "{{.Name}}"]),
    ] {
        left.extend(docker_lines(&args).await.into_iter().map(|n| format!("{what} {n}")));
    }
    left
}

/// The launch's values for a real VM lab: `CYBERCTF_TEST_VM_INPUTS` (`NAME=value,…`).
pub(crate) fn vm_inputs() -> Vec<(String, String)> {
    std::env::var("CYBERCTF_TEST_VM_INPUTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

/// `probe` run in the box, through the console the lab page opens, without a terminal.
pub(crate) async fn in_box(console: &ShellCommand, probe: &str) -> crate::error::Result<String> {
    let mut args = console.args.clone();
    match (console.program.as_str(), args.len()) {
        // `docker compose … exec <service> sh -c <shell>` (or `… exec <service> bash`): no TTY,
        // the probe for the shell.
        ("docker", _) => {
            let exec = args.iter().position(|a| a == "exec").expect("an exec");
            args.insert(exec + 1, "-T".into());
            if args.len() >= 2 && args[args.len() - 2] == "-c" {
                *args.last_mut().unwrap() = probe.into();
            } else {
                *args.last_mut().unwrap() = "sh".into();
                args.extend(["-c".to_owned(), probe.to_owned()]);
            }
        }
        // `vagrant ssh <vm>`: the probe as its command.
        ("vagrant", 2) => args.extend(["-c".to_owned(), probe.to_owned()]),
        // `ssh -t … <login shell>` into a VM: no TTY, the probe for the shell.
        ("ssh", _) if !args.last().is_some_and(|a| a.contains("docker exec")) => {
            args.retain(|a| a != "-t");
            *args.last_mut().unwrap() = probe.into();
        }
        // `vagrant ssh <vm> -c <into the container>`, `ssh -t … <into the container>`: no TTY
        // into it, the probe for its shell.
        ("vagrant" | "ssh", _) => {
            args.retain(|a| a != "-t");
            let into = args.last_mut().unwrap();
            let shell = into.find("sh -c '").expect("into a container's shell");
            *into = format!("{} sh -c '{probe}'", into[..shell].replace(" -it ", " -i "));
        }
        _ => panic!("an unexpected console: {console:?}"),
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let program = match console.program.as_str() {
        "docker" => "docker",
        "ssh" => "ssh",
        _ => "vagrant",
    };
    crate::platform::exec::run_env_timed(program, &args, console.cwd.as_deref(), &console.env, std::time::Duration::from_secs(120)).await
}

pub(crate) fn check(ok: bool, what: String, failures: &mut Vec<String>) {
    if ok {
        println!("ok: {what}");
    } else {
        println!("FAILED: {what}");
        failures.push(what);
    }
}
