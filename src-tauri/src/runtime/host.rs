//! Where a lab runs, and its run there: on this machine, on an ESXi or Proxmox server, or in a
//! cloud account, in the form it was started in. The one rule every operation on a started lab
//! (its status, a stop, park and resume, provisioning, its checks, the attack box) finds its run
//! by.

use std::path::Path;

use isothrum_runner::{ShellCommand, Status};

use super::paths::runner_home;
use super::providers::Provider;
use super::recorded::Recorded;
use super::{Runtime, docker, isothrum, lab, server};
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// A lab's run, where it runs: on this machine (on Docker, or as VMs), on an ESXi server (Vagrant there), on a Proxmox server
/// (its place for reading it; a change finds its node and SSH jump first), or in a cloud account
/// (its VMs, its containers on a VM, or its cloud services): where is its run's [`isothrum::Site`].
pub(crate) struct Host {
    /// The lab's run.
    pub(crate) run: isothrum::Deployment,
    /// The server's or the account's connection (its settings and secret); none on this machine.
    pub(crate) conn: Option<server::Connection>,
}

/// Lab `id`, installed in `dir` and declared `runtime`, where it runs and in the form it was
/// started in, as its run's record says ([`Recorded`]: a container lab started as one VM per
/// machine is VMs): on the server host or cloud account the record names, else on this machine.
/// `None` for a cloud-services lab on no account: nothing of it runs anywhere.
pub(crate) async fn locate(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<Host>> {
    let recorded = Recorded::of(dirs, id).await?;
    // Where the run is recorded first: a record naming a server no longer saved says so, whatever runs.
    let connection = recorded.host().map(|host| server::connection(dirs, host)).transpose()?;
    // A cloud-services lab runs nowhere in a build without the feature.
    if !cfg!(feature = "cloud-services") && runtime == Runtime::Cloud {
        return Ok(None);
    }
    let home = runner_home(dirs)?;
    let form = recorded.form(runtime);
    let run = match &connection {
        None => return Ok(isothrum::Deployment::local_of(&home, dir, id, runtime, &recorded)?.map(Host::here)),
        Some(conn) if conn.provider == Provider::VmwareEsxi => super::launch::esxi_run(dirs, dir, id, conn)?,
        Some(conn) if conn.provider.is_cloud() => {
            let place = isothrum::CloudPlace::of(conn);
            #[cfg(feature = "cloud-services")]
            let run = if runtime == Runtime::Cloud {
                isothrum::Deployment::own(&home, dir, id, place)?
            } else {
                isothrum::Deployment::in_cloud(&home, dir, id, (form, conn.terraform()?), place)?
            };
            #[cfg(not(feature = "cloud-services"))]
            let run = isothrum::Deployment::in_cloud(&home, dir, id, (form, conn.terraform()?), place)?;
            run
        }
        Some(conn) => isothrum::Deployment::on_server(&home, dir, id, form, isothrum::ServerPlace::of(conn))?,
    };
    Ok(Some(Host { run, conn: connection }))
}

/// Why changing a run failed, and whether the run may have changed by then.
pub(crate) struct Failed {
    /// What went wrong.
    pub(crate) error: Error,
    /// False only when the run certainly didn't change: its place couldn't be reached, or the
    /// runner refused the change before anything acted (its report's `changed`: a cloud sign-in
    /// that ran out included).
    pub(crate) run_changed: bool,
}

impl Failed {
    /// A failure of the launcher's own (a place's options not made), which may have changed the
    /// run.
    pub(crate) fn of_launcher(error: Error) -> Self {
        Failed { error, run_changed: true }
    }
}

impl Host {
    /// Lab run `run` on this machine.
    pub(crate) fn here(run: isothrum::Deployment) -> Host {
        Host { run, conn: None }
    }

    /// Whether the lab runs on this machine.
    pub(crate) fn is_here(&self) -> bool {
        self.conn.is_none()
    }

    /// Whether the lab runs on an ESXi server.
    pub(crate) fn on_esxi(&self) -> bool {
        matches!(self.run.site(), isothrum::Site::Esxi(_))
    }

    /// The Isoloom target the run's files are generated for; none for a module of the lab's own.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        self.run.target()
    }

    /// Whether the run has machines of the lab's own: every run but a cloud-services lab's,
    /// whose services are no machines to check or to put an attack box beside.
    pub(crate) fn has_machines(&self) -> bool {
        self.run.form() != isothrum::Form::Own
    }

    /// The run placed as a change or a console takes it, where that differs from how it is read:
    /// on a Proxmox server, its node and its SSH jump, found first.
    async fn placed(&self, dirs: &AppDirs) -> Result<Option<isothrum::Deployment>> {
        let (Some(conn), isothrum::Site::Server(_)) = (&self.conn, self.run.site()) else { return Ok(None) };
        let place = super::launch::server_place(dirs, &conn.id, conn).await?;
        Ok(Some(self.run.at(isothrum::Site::Server(place))))
    }

    /// Brings the run up to date with the lab's regenerated files (a machine added or taken out).
    pub(crate) async fn update(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        let placed = self.placed(dirs).await.map_err(|error| Failed { error, run_changed: false })?;
        placed.as_ref().unwrap_or(&self.run).update(log).await
    }

    /// Takes `machine` out of the run ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, dirs: &AppDirs, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.placed(dirs).await?.as_ref().unwrap_or(&self.run).retire(machine, log).await
    }

    /// Removes everything the run created, wherever it runs.
    pub(crate) async fn down(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        self.placed(dirs).await?.as_ref().unwrap_or(&self.run).down(log).await
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        self.run.run_status().await
    }

    /// The command that opens a shell into `machine`, as account `user` when given.
    pub(crate) async fn console(&self, machine: &str, user: Option<&str>) -> Result<ShellCommand> {
        self.run.console_command(machine, user).await
    }

    /// `machine`'s graphical console (its screen), wherever the lab runs.
    pub(crate) async fn graphical_console(&self, dirs: &AppDirs, machine: &str) -> Result<isothrum_client::HeldConsole> {
        self.placed(dirs).await?.as_ref().unwrap_or(&self.run).graphical_console(machine).await
    }

    /// What opens running VM `machine` in its hypervisor's own app, wherever the lab runs (a
    /// cloud's VMs have none).
    pub(crate) async fn native_console(&self, dirs: &AppDirs, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.placed(dirs).await?.as_ref().unwrap_or(&self.run).native_console(machine).await
    }

    /// Runs the lab's checks where it runs (a server's over SSH, an ESXi host's from its
    /// controller, a cloud-services lab's pages; a cloud's generated module none yet).
    pub(crate) async fn check(&self) -> Result<docker::Check> {
        self.run.check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_support::record_run;
    use crate::testing::test_app::TestApp;
    use crate::testing::tools::Tools;

    /// A lab with both forms: a container per machine, and a VM recipe for it.
    const SPEC: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, vm: { box: debian/bookworm64 }, networks: { lab: 10 } }\n";

    /// Where lab `t` runs, as its kind and the target its run's files are generated for.
    async fn located(t: &TestApp, dir: &Path, runtime: Runtime) -> Option<(&'static str, Option<lab::Target>)> {
        locate(&t.dirs(), dir, "t", runtime).await.unwrap().map(|h| {
            let kind = match (h.run.site(), &h.conn) {
                (isothrum::Site::Here, None) => "local",
                (isothrum::Site::Esxi(_), Some(_)) => "esxi",
                (isothrum::Site::Cloud(place), Some(_)) => {
                    assert_eq!(place.label, "acct", "the account's own id, from its connection");
                    "cloud"
                }
                (isothrum::Site::Server(_), Some(conn)) => {
                    assert_eq!(conn.id, "pve", "the server's own id, from its connection");
                    "server"
                }
                (site, conn) => panic!("{site:?} on {:?}", conn.as_ref().map(|c| &c.id)),
            };
            (kind, h.target())
        })
    }

    #[tokio::test]
    async fn a_lab_is_found_where_its_run_is_recorded_in_the_form_it_was_started_in() {
        let t = TestApp::new();
        let _tools = Tools::strict_rules(&[]);
        let dir = t.lab("t", &[("isoloom.yml", SPEC)]);
        // On this machine: as its runtime says, or as one VM per machine; a cloud-services lab on
        // no account runs nowhere.
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("local", Some(lab::Target::Docker))));
        assert_eq!(located(&t, &dir, Runtime::Vm).await, Some(("local", Some(lab::Target::Vagrant))));
        assert_eq!(located(&t, &dir, Runtime::Cloud).await, None);
        record_run(t.data(), "t", None, "vagrant", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("local", Some(lab::Target::Vagrant))));
        // In a cloud account: in the form it was started in, cloud services included.
        let acct = t.host("acct", "aws", serde_json::json!({ "useCliCreds": true }));
        record_run(t.data(), "t", Some(&acct), "cloud-vm/aws", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("cloud", Some(lab::Target::CloudVm))));
        record_run(t.data(), "t", Some(&acct), "cloud-docker/aws", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("cloud", Some(lab::Target::CloudDocker))));
        #[cfg(feature = "cloud-services")]
        assert_eq!(located(&t, &dir, Runtime::Cloud).await, Some(("cloud", None)), "its own module: no Isoloom target");
        #[cfg(not(feature = "cloud-services"))]
        assert_eq!(located(&t, &dir, Runtime::Cloud).await, None, "a build without the feature runs none");
        // On a server: Proxmox through Terraform, ESXi through Vagrant there.
        crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
        for server in ["pve", "esx"] {
            crate::runtime::server::secrets::set_secret(server, "pw").unwrap();
        }
        record_run(t.data(), "t", Some(&t.host("pve", "proxmox", serde_json::json!({}))), "proxmox", "ready");
        assert_eq!(located(&t, &dir, Runtime::Vm).await, Some(("server", Some(lab::Target::Proxmox))));
        record_run(t.data(), "t", Some(&t.host("esx", "vmware_esxi", serde_json::json!({}))), "vagrant", "ready");
        assert_eq!(located(&t, &dir, Runtime::Vm).await, Some(("esxi", Some(lab::Target::Vagrant))));
    }

    #[cfg(feature = "cloud-services")]
    #[tokio::test]
    async fn cloud_services_have_no_machines_to_check_nor_to_put_an_attack_box_beside() {
        let t = TestApp::new();
        let tools = Tools::strict_rules(&[]);
        let dir = t.lab("t", &[("isoloom.yml", SPEC), ("cloud/lab.yml", "provider: aws\n"), ("cloud/main.tf", "")]);
        record_run(t.data(), "t", Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true }))), "cloud-docker/aws", "ready");
        let host = locate(&t.dirs(), &dir, "t", Runtime::Cloud).await.unwrap().unwrap();
        assert!(!host.has_machines());
        let check = host.check().await.unwrap();
        assert!(!check.available && check.results.is_empty());
        assert!(crate::runtime::attack_box::locate(&t.dirs(), &dir, "t", Runtime::Cloud).await.unwrap().is_none());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        // Its containers in the same account are machines of the lab's own.
        assert!(locate(&t.dirs(), &dir, "t", Runtime::Docker).await.unwrap().unwrap().has_machines());
        assert!(crate::runtime::attack_box::locate(&t.dirs(), &dir, "t", Runtime::Docker).await.unwrap().is_some());
    }
}
