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

/// A lab's run, where it runs.
pub(crate) enum Host {
    /// On this machine: on Docker, or as VMs ([`isothrum::LabRun::local`]), or in the helper VM
    /// an earlier version ran a container lab inside.
    Local(isothrum::LabRun),
    /// VMs on an ESXi server (Vagrant there), through its connection.
    Esxi {
        /// The lab's run on the host.
        run: isothrum::LabRun,
        /// The host's connection: its settings and secret.
        conn: server::Connection,
    },
    /// In a cloud account (its VMs, its containers on a VM, or its cloud services), with the
    /// account's credentials (its run's place).
    Cloud {
        /// The lab's run in the account.
        run: isothrum::Deployment,
        /// The account's connection: its settings and credentials.
        conn: server::Connection,
    },
    /// On a Proxmox server, through `conn`: its place is found when the run is changed (its node,
    /// its SSH jump), not to read its status.
    Server {
        /// The lab's run on the server, placed for reading it.
        run: isothrum::Deployment,
        /// The server's connection: its settings and secret.
        conn: server::Connection,
    },
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
    Ok(Some(match connection {
        None => return Ok(isothrum::LabRun::local(&home, dir, id, runtime, &recorded)?.map(Host::Local)),
        Some(conn) if conn.provider == Provider::VmwareEsxi => Host::Esxi { run: super::launch::esxi_run(dirs, dir, id, form, &conn)?, conn },
        Some(conn) if conn.provider.is_cloud() => {
            let place = isothrum::CloudPlace::of(&conn);
            #[cfg(feature = "cloud-services")]
            let run = if runtime == Runtime::Cloud {
                isothrum::Deployment::own(&home, dir, id, place)?
            } else {
                isothrum::Deployment::in_cloud(&home, dir, id, (form, conn.terraform()?), place)?
            };
            #[cfg(not(feature = "cloud-services"))]
            let run = isothrum::Deployment::in_cloud(&home, dir, id, (form, conn.terraform()?), place)?;
            Host::Cloud { run, conn }
        }
        Some(conn) => Host::Server { run: isothrum::Deployment::on_server(&home, dir, id, form, isothrum::ServerPlace::of(&conn))?, conn },
    }))
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
    /// The Isoloom target the run's files are generated for; none for a container lab an earlier
    /// version runs inside a helper VM (here, on ESXi or on Proxmox), whose files Isoloom no
    /// longer generates: that run is only read, stopped and removed.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.target(),
            Host::Cloud { run, .. } | Host::Server { run, .. } => run.target(),
        }
    }

    /// Whether the run has machines of the lab's own: every run but a cloud-services lab's,
    /// whose services are no machines to check or to put an attack box beside.
    pub(crate) fn has_machines(&self) -> bool {
        !matches!(self, Host::Cloud { run, .. } if run.module() == isothrum::Module::Own)
    }

    /// The Proxmox server's run `run`, placed as a change or a console takes it (its node, its SSH
    /// jump).
    async fn placed(dirs: &AppDirs, run: &isothrum::Deployment, conn: &server::Connection) -> Result<isothrum::Deployment> {
        let place = super::launch::server_place(dirs, &conn.id, conn).await?;
        Ok(run.at(isothrum::Site::Server(place)))
    }

    /// Brings the run up to date with the lab's regenerated files (a machine added or taken out).
    pub(crate) async fn update(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> std::result::Result<(), Failed> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.update(log).await,
            Host::Cloud { run, .. } => run.update(log).await,
            Host::Server { run, conn } => Self::placed(dirs, run, conn).await.map_err(|error| Failed { error, run_changed: false })?.update(log).await,
        }
    }

    /// Takes `machine` out of the run ahead of its removal from the lab's files.
    pub(crate) async fn retire(&self, dirs: &AppDirs, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.retire(machine, log).await,
            Host::Cloud { run, .. } => run.retire(machine, log).await,
            Host::Server { run, conn } => Self::placed(dirs, run, conn).await?.retire(machine, log).await,
        }
    }

    /// Removes everything the run created, wherever it runs.
    pub(crate) async fn down(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.down(log).await,
            Host::Cloud { run, .. } => run.down(log).await,
            Host::Server { run, conn } => Self::placed(dirs, run, conn).await?.down(log).await,
        }
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<Status>> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.run_status().await,
            Host::Cloud { run, .. } | Host::Server { run, .. } => run.run_status().await,
        }
    }

    /// The command that opens a shell into `machine`, as account `user` when given.
    pub(crate) async fn console(&self, machine: &str, user: Option<&str>) -> Result<ShellCommand> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.console_command(machine, user).await,
            Host::Cloud { run, .. } | Host::Server { run, .. } => run.console_command(machine, user).await,
        }
    }

    /// `machine`'s graphical console (its screen), wherever the lab runs.
    pub(crate) async fn graphical_console(&self, dirs: &AppDirs, machine: &str) -> Result<isothrum_client::HeldConsole> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.graphical_console(machine).await,
            Host::Cloud { run, .. } => run.graphical_console(machine).await,
            Host::Server { run, conn } => Self::placed(dirs, run, conn).await?.graphical_console(machine).await,
        }
    }

    /// What opens running VM `machine` in its hypervisor's own app, wherever the lab runs (a
    /// cloud's VMs have none).
    pub(crate) async fn native_console(&self, dirs: &AppDirs, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.native_console(machine).await,
            Host::Cloud { run, .. } => run.native_console(machine).await,
            Host::Server { run, conn } => Self::placed(dirs, run, conn).await?.native_console(machine).await,
        }
    }

    /// Runs the lab's checks where it runs (a server's over SSH, an ESXi host's from its
    /// controller, a cloud-services lab's pages; a cloud's generated module none yet).
    pub(crate) async fn check(&self) -> Result<docker::Check> {
        match self {
            Host::Local(run) | Host::Esxi { run, .. } => run.check().await,
            Host::Cloud { run, .. } | Host::Server { run, .. } => run.check().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::Tools;
    use crate::runtime::test_support::record_run;
    use crate::testing::test_app::TestApp;

    /// A lab with both forms: a container per machine, and a VM recipe for it.
    const SPEC: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, vm: { box: debian/bookworm64 }, networks: { lab: 10 } }\n";

    /// Where lab `t` runs, as its kind and the target its run's files are generated for.
    async fn located(t: &TestApp, dir: &Path, runtime: Runtime) -> Option<(&'static str, Option<lab::Target>)> {
        locate(&t.dirs(), dir, "t", runtime).await.unwrap().map(|h| {
            let kind = match &h {
                Host::Local(_) => "local",
                Host::Esxi { .. } => "esxi",
                Host::Cloud { run, .. } => {
                    let isothrum::Site::Cloud(place) = run.site() else { panic!("a cloud run in a cloud account") };
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

    #[tokio::test]
    async fn a_lab_is_found_where_its_run_is_recorded_in_the_form_it_was_started_in() {
        let t = TestApp::new();
        let _tools = Tools::strict_rules(&[]);
        let dir = t.lab("t", &[("isoloom.yml", SPEC)]);
        // On this machine: as its runtime says, as one VM per machine, or in the helper VM an
        // earlier version left it in (no target: never generated again); a cloud-services lab on
        // no account runs nowhere.
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("local", Some(lab::Target::Docker))));
        assert_eq!(located(&t, &dir, Runtime::Vm).await, Some(("local", Some(lab::Target::Vagrant))));
        assert_eq!(located(&t, &dir, Runtime::Cloud).await, None);
        record_run(t.data(), "t", None, "docker-vm", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("local", None)));
        assert!(crate::runtime::attack_box::locate(&t.dirs(), &dir, "t", Runtime::Docker).await.unwrap().is_none(), "no box goes into it");
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
        // A container lab there is the helper VM an earlier version left, on either server.
        record_run(t.data(), "t", Some("esx"), "docker-vm", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("esxi", None)));
        record_run(t.data(), "t", Some("pve"), "docker-vm/proxmox", "ready");
        assert_eq!(located(&t, &dir, Runtime::Docker).await, Some(("server", None)));
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

    #[tokio::test]
    async fn only_a_container_box_on_this_machine_pulls_its_image_here() {
        let t = TestApp::new();
        let _tools = Tools::strict_rules(&[]);
        let dir = t.lab("t", &[("isoloom.yml", SPEC)]);
        let placed = async |runtime| crate::runtime::attack_box::locate(&t.dirs(), &dir, "t", runtime).await.unwrap().unwrap();
        assert!(placed(Runtime::Docker).await.pulls_image_here());
        assert!(!placed(Runtime::Vm).await.pulls_image_here(), "a Kali VM pulls no image");
        record_run(t.data(), "t", Some(&t.host("acct", "aws", serde_json::json!({ "useCliCreds": true }))), "cloud-docker/aws", "ready");
        assert!(!placed(Runtime::Docker).await.pulls_image_here(), "pulled in the account's VM");
    }
}
