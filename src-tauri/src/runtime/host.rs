//! Where a lab runs, and its run there: on this machine, on an ESXi or Proxmox server, or in a
//! cloud account, in the form it was started in. The one rule every operation on a started lab
//! (its status, a stop, park and resume, provisioning, its checks, the attack box) finds its run
//! by.

use std::path::Path;

use isothrum_runner::{ShellCommand, Status};

use super::paths::{local_runtime, runner_home};
use super::providers::Provider;
use super::{Runtime, docker, isothrum, lab, server};
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// A lab's run, where it runs.
pub(crate) enum Host {
    /// On this machine: on Docker, in one local VM, or as VMs ([`isothrum::LabRun::local`]).
    Local(isothrum::LabRun),
    /// VMs on an ESXi server (Vagrant there), through its connection.
    Esxi { run: isothrum::LabRun, conn: server::Connection },
    /// In a cloud account (its VMs, its containers on a VM, or its cloud services), with the
    /// account's credentials (`place`).
    Cloud { run: isothrum::CloudRun, place: isothrum::CloudPlace, conn: server::Connection },
    /// On a Proxmox server, through `conn`: its place is found when the run is changed (its node,
    /// its SSH jump), not to read its status.
    Server { run: isothrum::ServerRun, conn: server::Connection },
}

/// Lab `id`, installed in `dir` and declared `runtime`, where it runs and in the form it was
/// started in (`paths::local_runtime`: a container lab started as one VM per machine is VMs):
/// on the server host or cloud account its marker names, else on this machine. `None` for a
/// cloud-services lab on no account: nothing of it runs anywhere.
pub(crate) fn locate(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<Host>> {
    let home = runner_home(dirs)?;
    let form = local_runtime(dir, runtime);
    Ok(Some(match server::lab_connection(dirs, dir)? {
        None => return Ok(isothrum::LabRun::local(&home, dir, id, runtime)?.map(Host::Local)),
        Some(conn) if conn.provider == Provider::VmwareEsxi => Host::Esxi { run: super::launch::esxi_run(dirs, dir, id, form, &conn)?, conn },
        Some(conn) if conn.provider.is_cloud() => {
            let run = isothrum::CloudRun::new(&home, dir, id, form, conn.terraform()?)?;
            Host::Cloud { run, place: isothrum::CloudPlace::of(&conn), conn }
        }
        Some(conn) => Host::Server { run: isothrum::ServerRun::new(&home, dir, id, form)?, conn },
    }))
}

/// Why changing a run failed, and whether the run may have changed by then.
pub(crate) struct Failed {
    pub(crate) error: Error,
    /// False only when the run certainly didn't change: its place couldn't be reached, or the
    /// change was refused before acting ([`isothrum::errors::refused_before_acting`]: a cloud
    /// sign-in that ran out included).
    pub(crate) run_changed: bool,
}

impl Failed {
    fn of_runner(error: Error) -> Self {
        let run_changed = !isothrum::errors::refused_before_acting(&error);
        Failed { error, run_changed }
    }
}

impl Host {
    /// The Isoloom target the run's files are generated for.
    pub(crate) fn target(&self) -> lab::Target {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.target(),
            Host::Cloud { run, .. } => run.target(),
            Host::Server { run, .. } => run.target(),
        }
    }

    /// The Proxmox server's place for a change of its run (its node, its SSH jump).
    async fn server_place(dirs: &AppDirs, run: &isothrum::ServerRun, conn: &server::Connection) -> Result<isothrum::ServerPlace> {
        super::launch::server_place(dirs, &conn.id, conn, run).await
    }

    /// Brings the run up to date with the lab's regenerated files (a machine added or taken out).
    pub(crate) async fn update(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.update(log).await.map_err(Failed::of_runner),
            Host::Cloud { run, place, .. } => run.update(place, log).await.map_err(Failed::of_runner),
            Host::Server { run, conn } => {
                let place = Self::server_place(dirs, run, conn).await.map_err(|error| Failed { error, run_changed: false })?;
                run.update(&place, log).await.map_err(Failed::of_runner)
            }
        }
    }

    /// Takes `machine` out of the run ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, dirs: &AppDirs, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.retire(machine, log).await,
            Host::Cloud { run, place, .. } => run.retire(place, machine, log).await,
            Host::Server { run, conn } => run.retire(&Self::server_place(dirs, run, conn).await?, machine, log).await,
        }
    }

    /// Removes everything the run created, wherever it runs.
    pub(crate) async fn down(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.down(log).await,
            Host::Cloud { run, place, .. } => run.down(place, log).await,
            Host::Server { run, conn } => run.down(&Self::server_place(dirs, run, conn).await?, log).await,
        }
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.run_status().await,
            Host::Cloud { run, .. } => run.run_status().await,
            Host::Server { run, .. } => run.run_status().await,
        }
    }

    /// The command that opens a shell into `machine`.
    pub(crate) fn console(&self, machine: &str) -> Result<ShellCommand> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.console_command(machine),
            Host::Cloud { run, .. } => run.console_command(machine),
            Host::Server { run, .. } => run.console_command(machine),
        }
    }

    /// Runs the lab's checks where it runs (a cloud's and a server's over SSH, an ESXi host's
    /// from its controller).
    pub(crate) async fn check(&self) -> Result<docker::Check> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.check().await,
            Host::Cloud { run, place, .. } => run.check(place).await,
            Host::Server { run, .. } => run.check().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::Tools;
    use crate::runtime::paths::{mark_local_vm, mark_per_machine_vm};
    use crate::testing::test_app::TestApp;

    /// A lab with both forms: a container per machine, and a VM recipe for it.
    const SPEC: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, vm: { box: debian/bookworm64 }, networks: { lab: 10 } }\n";

    /// Where lab `t` runs, as its kind and the target its run's files are generated for.
    fn located(t: &TestApp, dir: &Path, runtime: Runtime) -> Option<(&'static str, lab::Target)> {
        locate(&t.dirs(), dir, "t", runtime).unwrap().map(|h| {
            let kind = match &h {
                Host::Local(_) => "local",
                Host::Esxi { .. } => "esxi",
                Host::Cloud { place, .. } => {
                    assert_eq!(place.label, "acct", "the account's own id, from its connection");
                    "cloud"
                }
                Host::Server { conn, .. } => {
                    assert_eq!(conn.id, "pve", "the server's own id, from its connection");
                    "server"
                }
            };
            (kind, h.target())
        })
    }

    #[test]
    fn a_lab_is_found_where_its_marker_says_in_the_form_it_was_started_in() {
        let t = TestApp::new();
        let _tools = Tools::rules(&[]);
        let dir = t.lab("t", &[("isoloom.yml", SPEC)]);
        // On this machine: as its runtime says, a container lab in its helper VM, or as one VM
        // per machine; a cloud-services lab on no account runs nowhere.
        assert_eq!(located(&t, &dir, Runtime::Docker), Some(("local", lab::Target::Docker)));
        assert_eq!(located(&t, &dir, Runtime::Vm), Some(("local", lab::Target::Vagrant)));
        assert_eq!(located(&t, &dir, Runtime::Cloud), None);
        mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Docker), Some(("local", lab::Target::DockerVm)));
        mark_local_vm(&dir, None).unwrap();
        mark_per_machine_vm(&dir, true).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Docker), Some(("local", lab::Target::Vagrant)));
        // In a cloud account: in the form it was started in, cloud services included.
        server::mark_lab(&dir, Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true })))).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Docker), Some(("cloud", lab::Target::CloudVm)));
        mark_per_machine_vm(&dir, false).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Docker), Some(("cloud", lab::Target::CloudDocker)));
        assert_eq!(located(&t, &dir, Runtime::Cloud), Some(("cloud", lab::Target::CloudServices)));
        // On a server: Proxmox through Terraform, ESXi through Vagrant there.
        crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
        for server in ["pve", "esx"] {
            crate::runtime::server::secrets::set_secret(server, "pw").unwrap();
        }
        server::mark_lab(&dir, Some(&t.host("pve", "proxmox", serde_json::json!({})))).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Vm), Some(("server", lab::Target::Proxmox)));
        server::mark_lab(&dir, Some(&t.host("esx", "vmware_esxi", serde_json::json!({})))).unwrap();
        assert_eq!(located(&t, &dir, Runtime::Vm), Some(("esxi", lab::Target::Vagrant)));
    }
}
