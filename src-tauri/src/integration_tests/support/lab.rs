//! A lab installed for a test and run through the Isothrum runner, the launch's values a VM lab
//! gets, and what a lab leaves on Docker.

use std::path::{Path, PathBuf};

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
    /// The lab installed in `dir` under id `id`, its runner home in the temp folder beside it.
    pub(crate) fn in_place(dir: PathBuf, id: String) -> Run {
        let home = std::env::temp_dir().join(format!("cyberctf-{id}-home"));
        Run { dir, id, home }
    }

    /// A fresh copy of the lab in `src` ([`install_copy`]) under id `id`.
    pub(crate) fn install(src: &Path, id: String) -> Run {
        Run::in_place(install_copy(src, &id), id)
    }

    /// A fresh copy of the lab in `src` under an id of its own, `<prefix>-<n>`, so a scenario's
    /// run never meets one an earlier run left.
    pub(crate) fn fresh(src: &Path, prefix: &str) -> Run {
        Run::install(src, format!("{prefix}-{}", rand::random::<u16>()))
    }

    /// Removes what the scenario made on disk for the lab: its folder and its runner home. Best
    /// effort: what is already gone, or can't go, is left.
    pub(crate) fn remove_files(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.home);
    }

    /// Starts the lab with the launch's values `env`.
    pub(crate) async fn start(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        launch::start_docker_run(&self.home, &self.dir, &self.id, launch::Inputs::values(env), log).await
    }

    pub(crate) async fn status(&self) -> Result<LabStatus> {
        isothrum::Deployment::docker(&self.home, &self.dir, &self.id)?.status().await
    }

    pub(crate) async fn check(&self) -> Result<docker::Check> {
        isothrum::Deployment::docker(&self.home, &self.dir, &self.id)?.check().await
    }

    /// Parks the lab in `mode`, as the lab page's Pause (its containers frozen) or Shut down (its
    /// containers stopped, kept) does.
    pub(crate) async fn park(&self, mode: crate::runtime::Park, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::Deployment::docker(&self.home, &self.dir, &self.id)?.park(mode, log).await
    }

    /// Starts the parked lab again, as it was.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::Deployment::docker(&self.home, &self.dir, &self.id)?.resume(log).await
    }

    pub(crate) async fn stop(&self, log: impl FnMut(String)) -> Result<()> {
        lifecycle::stop_run(isothrum::Deployment::docker(&self.home, &self.dir, &self.id)?, &self.dir, log).await
    }
}

/// Whether the lab reads as up in `s`: running, and every machine of its own (not its plumbing)
/// running.
pub(crate) fn every_machine_runs(s: &LabStatus) -> bool {
    s.running && !s.machines.is_empty() && s.machines.iter().filter(|m| !m.infra).all(|m| m.state == "running")
}

/// A fresh copy of the lab, as the launcher installs one: its own folder, nothing generated.
/// Copied in Rust, not with `cp -R`, so the test runs on Windows too (R7).
pub(crate) fn install_copy(src: &Path, id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    crate::runtime::lab::copy_tree(src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    dir
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
