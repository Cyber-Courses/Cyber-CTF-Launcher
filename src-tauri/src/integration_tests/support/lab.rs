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
    /// Starts the lab with the launch's values `env`.
    pub(crate) async fn start(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        launch::start_docker_run(&self.home, &self.dir, &self.id, launch::Inputs::values(env), log).await
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
        lifecycle::stop_run(isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?, &self.dir, log).await
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
