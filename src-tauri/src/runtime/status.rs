//! What a lab looks like right now, wherever it runs: its machines and networks, where it runs,
//! whether it is parked, and (for a container lab on a Terraform host) the containers read from
//! that host.

use std::path::Path;

use crate::AppHandle;

use super::paths::{lab_dir, local_vm, parked, runner_home, vagrant_dirs};
use super::{LabStatus, Place, Runtime, docker, isothrum, server, vm};
use crate::error::Result;

pub(super) async fn status(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime) -> Result<LabStatus> {
    let status = match (runtime, server::lab_connection(app, dir)?) {
        // On a server host or cloud account: the runner's view.
        (_, Some(c)) => server_status(app, dir, id, runtime, c).await?,
        // Cloud-services labs run only on an account; nothing of them on this machine.
        (Runtime::Cloud, None) => LabStatus::default(),
        // On this machine, through the runner: a container lab on Docker or in one local VM, or a
        // VM lab. Parked = the launcher parked it and it is still down.
        (Runtime::Docker, None) if local_vm(dir).is_some() => {
            with_parked(isothrum::LabRun::docker_in_vm(&runner_home(app)?, dir, id)?.status().await?, parked(dir))
        }
        (Runtime::Docker, None) => with_parked(isothrum::LabRun::docker(&runner_home(app)?, dir, id)?.status().await?, parked(dir)),
        (Runtime::Vm, None) => with_parked(isothrum::LabRun::vm(&runner_home(app)?, dir, id)?.status().await?, parked(dir)),
    };
    // A deployment the auto-stop couldn't destroy, wherever it is: said on the lab's page.
    Ok(LabStatus { reap_error: super::reaper::last_failure(app, id), ..status })
}

/// Parked = the launcher parked it (`parked`) and it is still down; a lab someone started again
/// by hand (or that is up for any reason) is simply running.
fn with_parked(s: LabStatus, parked: Option<super::Park>) -> LabStatus {
    LabStatus { parked: parked.filter(|_| !s.running), ..s }
}

async fn server_status(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, c: server::Connection) -> Result<LabStatus> {
    // A setup step that failed can leave a lab's machines up but the lab not ready: not running.
    let failed = crate::deploy_worker::last_deploy_failed(app, id);
    let seen = |s: LabStatus| LabStatus { running: s.running && !failed, ..s };
    // ESXi (Vagrant on the host): the runner's view, Vagrant's states, each VM at its own address.
    if c.provider == super::providers::Provider::VmwareEsxi {
        let status = super::launch::esxi_run(app, dir, id, runtime, &c)?.status().await?;
        return Ok(on_server(seen(status), c.name, c.provider));
    }
    // A cloud account (cloud-vm, cloud-docker, cloud-services): the runner's view from its record.
    if c.provider.is_cloud() {
        let cloud = server::terraform_target(c.provider).unwrap_or_default();
        let status = isothrum::CloudRun::new(&runner_home(app)?, dir, id, runtime, cloud)?.status().await?;
        return Ok(on_server(seen(status), c.name, c.provider));
    }
    // A Proxmox server: the runner's view from its own record and state (no SSH, no API call);
    // `ServerRun::status` fills the host and provider.
    let place = isothrum::ServerPlace { name: c.name.clone(), ..isothrum::ServerPlace::default() };
    let status = isothrum::ServerRun::new(&runner_home(app)?, dir, id, runtime)?.status(&place).await?;
    Ok(seen(status))
}

/// A lab's status as running on the server host `name` (a cloud account, or a server).
fn on_server(status: LabStatus, name: String, provider: super::providers::Provider) -> LabStatus {
    let place = if provider.is_cloud() { Place::Cloud } else { Place::Server };
    LabStatus { host: Some(name), place: Some(place), provider: Some(provider.id().to_string()), ..status }
}

/// Whether the lab runs on this machine right now: its containers, or a local VM of it. An
/// upgrade must not swap the folder out from under it (its containers bind-mount files from it).
pub async fn running_here(dir: &Path, id: &str) -> bool {
    if docker::status(dir, id).await.is_ok_and(|s| s.running) {
        return true;
    }
    for vdir in vagrant_dirs(dir) {
        if vm::status(&vdir, &[]).await.is_ok_and(|s| s.running) {
            return true;
        }
    }
    false
}

/// Whether Vagrant still tracks machines of this lab on this machine, running or not: their
/// record lives in the lab's folder, which must not be replaced under them.
pub fn vms_recorded_here(dir: &Path) -> bool {
    vagrant_dirs(dir).into_iter().any(|vdir| vm::has_machines(&vdir))
}

/// Whether lab `id` is up somewhere before a start: running here, parked here, or marked as
/// running on a server host or cloud account. What a failed start finds already up was not
/// brought up by it, and must not be torn down with what it did bring up. A leftover (a state
/// left on another account, stopped VMs the start clears) doesn't count: a failed start tears
/// down its own deployment as before. False when it isn't installed.
pub async fn lab_present(app: &AppHandle, id: &str) -> bool {
    let Ok(dir) = lab_dir(app, id) else { return false };
    server::marked_host(&dir).is_some_and(|h| !h.is_empty()) || parked(&dir).is_some() || running_here(&dir, id).await
}

/// Whether lab `id` is installed here.
pub fn lab_installed(app: &AppHandle, id: &str) -> bool {
    lab_dir(app, id).is_ok()
}

/// Where a running lab is reachable on this machine (its first published port). None for
/// VM labs (their address is discovered differently) or when nothing is published.
pub async fn primary_url(dir: &Path, id: &str, runtime: Runtime) -> Option<String> {
    match runtime {
        Runtime::Docker if !dir.join(server::HOST_MARKER).exists() && local_vm(dir).is_none() => docker::primary_url(dir, id).await,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Place, on_server, primary_url, with_parked};
    use crate::runtime::providers::Provider;
    use crate::runtime::test_support::status_with;
    use crate::runtime::{Park, Runtime};

    #[test]
    fn only_a_lab_that_is_down_reads_as_parked() {
        assert_eq!(with_parked(status_with(false, &[]), Some(Park::Pause)).parked, Some(Park::Pause));
        assert_eq!(with_parked(status_with(true, &["web"]), Some(Park::Pause)).parked, None, "started again by hand");
        assert_eq!(with_parked(status_with(false, &[]), None).parked, None);
    }

    #[test]
    fn a_server_lab_says_where_it_runs() {
        let s = on_server(status_with(true, &["web"]), "My AWS".into(), Provider::Aws);
        assert_eq!((s.host.as_deref(), s.provider.as_deref()), (Some("My AWS"), Some("aws")));
        assert!(matches!(s.place, Some(Place::Cloud)));
        let s = on_server(status_with(true, &["web"]), "pve".into(), Provider::Proxmox);
        assert!(matches!(s.place, Some(Place::Server)));
        assert_eq!(s.provider.as_deref(), Some("proxmox"));
    }

    #[tokio::test]
    async fn only_a_local_container_lab_has_a_url_here() {
        let dir = std::env::temp_dir().join(format!("cyberctf-status-url-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        // A VM lab, a lab on a server host, a container lab in a local VM: no Docker probe at all.
        assert_eq!(primary_url(&dir, "t", Runtime::Vm).await, None);
        crate::runtime::paths::mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        assert_eq!(primary_url(&dir, "t", Runtime::Docker).await, None);
        crate::runtime::paths::mark_local_vm(&dir, None).unwrap();
        std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "h").unwrap();
        assert_eq!(primary_url(&dir, "t", Runtime::Docker).await, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_lab_is_present_wherever_something_of_it_is_left() {
        // Regression: a failed website launch asked only whether the lab ran *here* (local Docker
        // and Vagrant), so a lab up on a server host or cloud account read as absent, and the
        // teardown of the failed start destroyed it.
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let app = t.handle();
        let _tools = Tools::rules(&[]);
        let dir = t.lab("present", &[]);
        assert!(!super::lab_present(app, "present").await, "installed, nothing of it up");
        // Marked as running on a host.
        crate::runtime::server::mark_lab(&dir, Some("pve1")).unwrap();
        assert!(super::lab_present(app, "present").await);
        crate::runtime::server::mark_lab(&dir, None).unwrap();
        // A Terraform state left on another account isn't this lab being up: a failed start on
        // another account still tears down what it created there.
        let state = t.data().join("deployments/present/aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
        assert!(!super::lab_present(app, "present").await);
        assert!(super::lab_installed(app, "present") && !super::lab_installed(app, "gone"));
        // Parked here.
        crate::runtime::paths::mark_parked(&dir, Some(crate::runtime::Park::Pause)).unwrap();
        assert!(super::lab_present(app, "present").await);
    }

    #[tokio::test]
    async fn a_lab_with_nothing_up_is_not_running_here() {
        use crate::platform::exec::fake::Tools;
        let dir = std::env::temp_dir().join(format!("cyberctf-status-here-{}", rand::random::<u32>()));
        let vagrant = crate::runtime::lab::vagrant_dir(&dir, Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), "").unwrap();
        let tools = Tools::rules(&[]);
        assert!(!super::running_here(&dir, "t").await);
        assert!(tools.called("vagrant status"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_lab_marked_on_a_host_that_is_no_longer_saved_names_that_host() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("web", &[]);
        std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "gone1\n").unwrap();
        let tools = Tools::rules(&[]);
        for runtime in [Runtime::Docker, Runtime::Vm, Runtime::Cloud] {
            let err = super::status(t.handle(), &dir, "web", runtime).await.err().expect("refused").to_string();
            assert_eq!(err, "server host `gone1` not found", "{runtime:?}");
        }
        // Refused before anything local or remote is probed; the marker alone never makes it
        // run here either.
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(!super::running_here(&dir, "web").await);
    }

    #[tokio::test]
    async fn running_here_is_false_when_neither_docker_nor_vagrant_answers() {
        use crate::platform::exec::fake::{Tools, fail};
        let dir = std::env::temp_dir().join(format!("cyberctf-status-down-{}", rand::random::<u32>()));
        let vagrant = crate::runtime::lab::vagrant_dir(&dir, Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), "").unwrap();
        // A wedged engine and a wedged hypervisor: an error, not a lab that reads as running.
        let tools = Tools::rules(&[("docker", fail("Cannot connect to the Docker daemon")), ("vagrant status", fail("VBoxManage: error: timed out"))]);
        assert!(!super::running_here(&dir, "t").await);
        assert!(tools.called("docker compose") && tools.called("vagrant status"), "{:?}", tools.calls());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_parked_mark_is_reported_only_while_the_lab_is_down_whatever_the_mode() {
        for mode in [Park::Pause, Park::Shutdown] {
            assert_eq!(with_parked(status_with(false, &[]), Some(mode)).parked, Some(mode));
            // Half up (one machine running by hand): the status is "running", never "parked",
            // so the page offers Stop rather than a Resume that would do nothing.
            assert_eq!(with_parked(status_with(true, &["dc01"]), Some(mode)).parked, None);
        }
    }
}
