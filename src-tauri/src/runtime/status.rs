//! What a lab looks like right now, wherever it runs: its machines and networks, where it runs,
//! whether it is parked, and (for a container lab on a Terraform host) the containers read from
//! that host.

use std::path::Path;

use crate::platform::AppDirs;

use super::host::{self, Host};
use super::paths::{lab_dir, local_runtime, local_vm, parked, per_machine_vm};
use super::providers::Provider;
use super::{LabStatus, Place, Runtime, attack_machine, docker, isothrum, server};
use crate::error::{Error, Result};

pub(crate) async fn status(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<LabStatus> {
    // Where it runs, in the form it was started in (a container lab run as one VM per machine
    // reads as a VM lab): the runner's view. A cloud-services lab on no account runs nowhere.
    let host = host::locate(dirs, dir, id, runtime)?;
    let status = match &host {
        Some(host) => status_at(host).await?,
        None => LabStatus::default(),
    };
    let here = matches!(host, Some(Host::Local(_)));
    // What the page may do on it, by the lifecycle's own rules.
    let controls = super::lifecycle::controls(host.as_ref(), status.provider.as_deref());
    // The attack box the launcher added is no machine of the lab's own (`attack_machine`).
    let status = attack_machine::set_apart(status, dir)?;
    // Here, parked = the launcher parked it and it is still down.
    let status = if here { with_parked(status, parked(dir)) } else { status };
    // Whether this container lab runs as one VM per machine (its VM recipes), locally or as
    // cloud-vm: the UI reads it to name the form.
    let per_machine = runtime == Runtime::Docker && per_machine_vm(dir);
    // A deployment the auto-stop couldn't destroy, wherever it is: said on the lab's page.
    let reap_error = super::reaper::last_failure(dirs, id);
    Ok(LabStatus { per_machine, reap_error, controls, ..status })
}

/// Parked = the launcher parked it (`parked`) and it is still down; a lab someone started again
/// by hand (or that is up for any reason) is simply running.
fn with_parked(s: LabStatus, parked: Option<super::Park>) -> LabStatus {
    LabStatus { parked: parked.filter(|_| !s.running), ..s }
}

/// The lab as the runner sees it where it runs. The failed-launch masking is applied by `status`,
/// uniformly, so it is not repeated here.
async fn status_at(host: &Host) -> Result<LabStatus> {
    Ok(match host {
        // On this machine: a container lab on Docker or in one local VM, or a VM lab.
        Host::Local(run) => run.status().await?,
        // ESXi (Vagrant on the host): Vagrant's states, each VM at its own address.
        Host::Esxi { run, conn } => on_server(run.status().await?, conn),
        // A cloud account (cloud-vm, cloud-docker, cloud-services): from the runner's record.
        Host::Cloud { run, conn, .. } => on_server(run.status().await?, conn),
        // A Proxmox server: from the runner's own record and state (no SSH, no API call).
        Host::Server { run, conn } => {
            on_server(run.status(&isothrum::ServerPlace { name: conn.name.clone(), ..isothrum::ServerPlace::default() }).await?, conn)
        }
    })
}

/// A lab's status as running on the server host or cloud account of connection `conn`: its
/// name, its id (the saved host's, which the page finds it by) and its provider.
fn on_server(status: LabStatus, conn: &server::Connection) -> LabStatus {
    let place = if conn.provider.is_cloud() { Place::Cloud } else { Place::Server };
    LabStatus { host: Some(conn.name.clone()), host_id: Some(conn.id.clone()), place: Some(place), provider: Some(conn.provider.id().to_string()), ..status }
}

/// Something of a lab still up somewhere: its run, in any state but gone (running, starting, a
/// failed start's machines left over, parked), or a deployment of the launcher's own Terraform
/// path not handed to the runner yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Up {
    /// The saved server or cloud account it is on, by id; `None`: this machine.
    pub(crate) host: Option<String>,
    /// That host's name, as the player reads it (its id when it is no longer saved).
    pub(crate) host_name: Option<String>,
    /// Only parked: nothing of it runs, its machines are kept.
    pub(crate) parked: bool,
}

/// Whether something of lab `id` is still up, and where: its run as the runner records it (run
/// `cyberctf-<id>`, in the state its last operation left it in, on the place it was started on:
/// this machine, a server or a cloud account, whatever the form), or a deployment of the
/// launcher's own Terraform path still waiting for its hand-over to the runner
/// (`isothrum::import`). The one rule an update, a re-import, a start somewhere else, a failed
/// website launch and a host's removal go by.
///
/// The runner keeps its record of a run (and Vagrant's) under the app data folder's `runs/`,
/// never in the lab folder, so the lab folder says nothing of it. The record is read from files
/// only: no tool runs and no host is reached. A record that can't be read is an error, never
/// "nothing up".
pub(crate) fn still_up(dirs: &AppDirs, id: &str) -> Result<Option<Up>> {
    // Named as the player reads it: by its id when it is no longer saved.
    let up = |host: Option<String>, parked: bool| -> Result<Up> {
        let host_name = match &host {
            Some(h) => Some(server::host_name(dirs, h)?.unwrap_or_else(|| h.clone())),
            None => None,
        };
        Ok(Up { host, host_name, parked })
    };
    if let Some(host) = awaiting_hand_over(dirs, id)? {
        return up(Some(host), false).map(Some);
    }
    let run = isothrum::record_of(dirs, id)?;
    run.filter(|r| r.state != isothrum_runner::RunState::Absent).map(|r| up(r.host_label, r.state == isothrum_runner::RunState::Parked)).transpose()
}

/// The host a deployment of lab `id` on the launcher's own Terraform path is on, while one still
/// holds resources: one the runner hasn't taken over yet (`isothrum::import`). The launcher no
/// longer runs Terraform itself, so nothing in it removes such a deployment until the runner
/// takes it over ([`left_awaiting_hand_over`]). Named by the host its state records, else by the
/// lab's marker. A deployment or state that can't be read is an error, never "nothing up": a
/// missed one is a billable leftover.
fn awaiting_hand_over(dirs: &AppDirs, id: &str) -> Result<Option<String>> {
    Ok(deployment_awaiting_hand_over(dirs, id)?.and_then(|(host, _)| host))
}

/// [`awaiting_hand_over`], with the provider of the deployment's state folder.
fn deployment_awaiting_hand_over(dirs: &AppDirs, id: &str) -> Result<Option<(Option<String>, Option<Provider>)>> {
    let base = dirs.data()?;
    for (target, state) in super::paths::deployment_states(base, id)? {
        if super::terraform::deployed(&state)? {
            let marked = || lab_dir(dirs, id).ok().and_then(|dir| server::marked_host(&dir)).filter(|h| !h.is_empty());
            return Ok(Some((super::paths::recorded_host(&state).or_else(marked), server::terraform_provider(&target))));
        }
    }
    Ok(None)
}

/// After a Stop of lab `id`, the deployment of the launcher's own Terraform path it leaves up (one
/// the runner hasn't taken over yet: nothing in the launcher removes it), said as an error that
/// tells the player how to remove it by hand. None when there is none.
pub(crate) fn left_awaiting_hand_over(dirs: &AppDirs, id: &str) -> Result<Option<Error>> {
    let Some((host, provider)) = deployment_awaiting_hand_over(dirs, id)? else { return Ok(None) };
    let place = match &host {
        Some(h) => server::host_name(dirs, h)?.unwrap_or_else(|| h.clone()),
        None => "a server or cloud account".into(),
    };
    let by_hand = provider.map(super::reaper::remove_by_hand).unwrap_or_default();
    Ok(Some(Error::Invalid(format!(
        "Part of this lab is still up on {place}: a deployment from an earlier version of the launcher that the Isothrum runner hasn't taken over, which the launcher can't remove. {by_hand} {}",
        super::reaper::RETRIED
    ))))
}

/// Refuses to replace lab `id`'s folder (`dir`), with a new version or a re-import, while
/// something of it is up ([`still_up`]): the run's manifest and the files it bind-mounts or
/// provisions from are in that folder. A parked copy is no reason: it is taken down before the
/// swap (`clear_parked`).
pub(crate) fn refuse_replace(dirs: &AppDirs, dir: &Path, id: &str) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    match still_up(dirs, id)? {
        Some(up) if up.parked && parked(dir).is_some() => Ok(()),
        Some(up) => Err(still_running(&up)),
        None => Ok(()),
    }
}

/// The refusal of an operation that needs the lab down while it is `up`.
pub(crate) fn still_running(up: &Up) -> crate::error::Error {
    use crate::error::{Code, Error};
    Error::coded(match &up.host_name {
        Some(host) => Code::LabRunningOn { host: host.clone() },
        None => Code::LabRunningHere,
    })
}

/// Whether lab `id` is up somewhere before a start: parked here, or up where its run is
/// ([`still_up`]). What a failed start finds already up was not brought up by it, and must not
/// be torn down with what it did bring up. When the runner's record can't be read, a lab marked
/// as running on a host counts as up there. False when it isn't installed.
pub fn lab_present(dirs: &AppDirs, id: &str) -> bool {
    let Ok(dir) = lab_dir(dirs, id) else { return false };
    if parked(&dir).is_some() {
        return true;
    }
    match still_up(dirs, id) {
        Ok(up) => up.is_some(),
        Err(e) => {
            let marked = server::marked_host(&dir).filter(|h| !h.is_empty());
            log::warn!("couldn't tell whether {id} is up ({e}); taken for {} by its host marker", if marked.is_some() { "up" } else { "down" });
            marked.is_some()
        }
    }
}

/// Whether lab `id` is installed here.
pub fn lab_installed(dirs: &AppDirs, id: &str) -> bool {
    lab_dir(dirs, id).is_ok()
}

/// Where a running lab is reachable on this machine (its first published port). None for
/// VM labs (their address is discovered differently) or when nothing is published.
pub async fn primary_url(dir: &Path, id: &str, runtime: Runtime) -> Option<String> {
    match local_runtime(dir, runtime) {
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

    /// The connection of saved host `id`, named `name`, on `provider`.
    fn conn(id: &str, name: &str, provider: Provider) -> crate::runtime::server::Connection {
        crate::runtime::server::Connection {
            id: id.into(),
            provider,
            name: name.into(),
            env: Vec::new(),
            tf_vars: Vec::new(),
            tf_env: Vec::new(),
            region: None,
            auto_stop_hours: None,
        }
    }

    #[test]
    fn a_server_lab_says_where_it_runs() {
        let s = on_server(status_with(true, &["web"]), &conn("h-aws", "My AWS", Provider::Aws));
        assert_eq!((s.host.as_deref(), s.host_id.as_deref(), s.provider.as_deref()), (Some("My AWS"), Some("h-aws"), Some("aws")));
        assert!(matches!(s.place, Some(Place::Cloud)));
        let s = on_server(status_with(true, &["web"]), &conn("h-pve", "pve", Provider::Proxmox));
        assert!(matches!(s.place, Some(Place::Server)));
        assert_eq!((s.host_id.as_deref(), s.provider.as_deref()), (Some("h-pve"), Some("proxmox")));
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

    #[test]
    fn a_lab_is_present_wherever_something_of_it_is_left() {
        // Regression: a failed website launch asked only whether the lab ran *here* (local Docker
        // and the lab folder's Vagrant record), so a lab up on a server host or cloud account, or
        // as VMs the runner started here, read as absent, and the failed start's teardown
        // destroyed it.
        use crate::runtime::test_support::record_run;
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let dir = t.lab("present", &[]);
        assert!(!super::lab_present(&dirs, "present"), "installed, nothing of it up");
        // Its run, wherever the runner records it up: in a cloud account, as VMs here.
        record_run(t.data(), "present", Some("aws1"), "cloud-docker", "ready");
        assert!(super::lab_present(&dirs, "present"));
        record_run(t.data(), "present", None, "vagrant", "failed");
        assert!(super::lab_present(&dirs, "present"), "a failed start's VMs left here");
        // Taken down: absent.
        record_run(t.data(), "present", None, "vagrant", "absent");
        assert!(!super::lab_present(&dirs, "present"));
        // A Terraform state of the launcher's own path still holding resources, waiting for its
        // hand-over to the runner, is the lab up on that account.
        let state = t.data().join("deployments/present/aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
        std::fs::write(state.join("host"), "aws1").unwrap();
        assert!(super::lab_present(&dirs, "present"));
        std::fs::remove_dir_all(t.data().join("deployments")).unwrap();
        assert!(super::lab_installed(&dirs, "present") && !super::lab_installed(&dirs, "gone"));
        // Parked here.
        crate::runtime::paths::mark_parked(&dir, Some(crate::runtime::Park::Pause)).unwrap();
        assert!(super::lab_present(&dirs, "present"));
    }

    #[test]
    fn a_lab_whose_run_cant_be_read_is_judged_by_its_marker_and_says_why() {
        // Regression: why the run couldn't be read was dropped, and the failed start's teardown
        // was decided on the marker alone with nothing in the log.
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("present", &[]);
        let state = t.data().join("deployments/present/aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), "not a state").unwrap();
        crate::testing::logs::take_warnings();
        assert!(!super::lab_present(&t.dirs(), "present"), "no marker: down");
        std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "aws1").unwrap();
        assert!(super::lab_present(&t.dirs(), "present"), "marked as on a host: up there");
        let logged = crate::testing::logs::take_warnings();
        assert_eq!(logged.len(), 2, "{logged:?}");
        assert!(logged[0].starts_with("couldn't tell whether present is up (") && logged[0].ends_with("taken for down by its host marker"), "{logged:?}");
        assert!(logged[1].ends_with("taken for up by its host marker"), "{logged:?}");
    }

    #[test]
    fn what_is_still_up_is_read_from_the_runners_record_wherever_it_runs() {
        // Regression: an update, a re-import and a start elsewhere asked the lab folder (Vagrant's
        // `.vagrant`, local Docker, `deployments/`), where the runner keeps nothing: a VM, a
        // docker-in-VM, an ESXi, a Proxmox or a cloud lab run by the runner read as down.
        use crate::runtime::test_support::record_run;
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let dir = t.lab("web", &[]);
        let pve = t.host("pve1", "proxmox", serde_json::json!({}));
        assert_eq!(super::still_up(&dirs, "web").unwrap(), None, "never started");
        for (host, target) in
            [(None, "docker"), (None, "docker-vm"), (None, "vagrant"), (Some("esx1"), "vagrant"), (Some(pve.as_str()), "proxmox"), (Some("aws1"), "cloud-vm")]
        {
            record_run(t.data(), "web", host, target, "ready");
            let up = super::still_up(&dirs, "web").unwrap().unwrap_or_else(|| panic!("{target} on {host:?}"));
            assert_eq!((up.host.as_deref(), up.parked), (host, false), "{target}");
            let err = super::refuse_replace(&dirs, &dir, "web").unwrap_err();
            let expected = match host {
                None => crate::error::Code::LabRunningHere,
                // A saved host by its name; one no longer saved by its id.
                Some(h) => crate::error::Code::LabRunningOn { host: if h == pve { "Host pve1".into() } else { h.into() } },
            };
            assert_eq!(err.code(), expected, "{target}");
            assert!(err.is_refusal());
        }
        // Parked: only its parked mark lets the copy go (it is taken down before the swap).
        record_run(t.data(), "web", None, "vagrant", "parked");
        assert!(super::still_up(&dirs, "web").unwrap().is_some_and(|u| u.parked));
        assert!(super::refuse_replace(&dirs, &dir, "web").is_err());
        crate::runtime::paths::mark_parked(&dir, Some(crate::runtime::Park::Shutdown)).unwrap();
        assert!(super::refuse_replace(&dirs, &dir, "web").is_ok());
        // Down: free to replace; another lab's run is no business of this one's.
        record_run(t.data(), "web", None, "vagrant", "absent");
        record_run(t.data(), "web-2", None, "docker", "ready");
        assert_eq!(super::still_up(&dirs, "web").unwrap(), None);
        assert!(super::refuse_replace(&dirs, &dir, "web").is_ok());
        // A record that can't be read stops the replacement rather than read as "down".
        std::fs::write(t.data().join("runs/cyberctf-web/record.json"), "not json").unwrap();
        assert!(super::still_up(&dirs, "web").is_err() && super::refuse_replace(&dirs, &dir, "web").is_err());
        // Nothing installed: nothing to replace.
        assert!(super::refuse_replace(&dirs, &t.data().join("labs/none"), "none").is_ok());
    }

    #[test]
    fn a_deployment_that_cant_be_read_is_an_error_never_nothing_up() {
        // Regression: an unreadable deployments folder or Terraform state read as "nothing up",
        // contradicting `still_up`'s own rule: a missed deployment is a billable leftover.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        t.lab("web", &[]);
        let state = t.data().join("deployments/web/aws");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("terraform.tfstate"), "{ cut short").unwrap();
        assert!(super::still_up(&dirs, "web").is_err(), "a state that can't be parsed");
        std::fs::write(state.join("terraform.tfstate"), r#"{"resources":[]}"#).unwrap();
        assert_eq!(super::still_up(&dirs, "web").unwrap(), None, "an empty state holds nothing");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let folder = t.data().join("deployments/web");
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
            let unreadable = super::still_up(&dirs, "web");
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(unreadable.is_err(), "a deployments folder that can't be listed");
        }
    }

    #[tokio::test]
    async fn a_lab_marked_on_a_host_that_is_no_longer_saved_names_that_host() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("web", &[]);
        std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "gone1\n").unwrap();
        let tools = Tools::rules(&[]);
        for runtime in [Runtime::Docker, Runtime::Vm, Runtime::Cloud] {
            let err = super::status(&t.dirs(), &dir, "web", runtime).await.err().expect("refused").to_string();
            assert_eq!(err, "server host `gone1` not found", "{runtime:?}");
        }
        // Refused before anything local or remote is probed; the marker alone never makes it
        // run here either.
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
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

    #[tokio::test]
    async fn the_status_carries_what_the_page_may_do_where_the_lab_runs() {
        use crate::runtime::Control;
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        let dir = t.lab("web", &[]);
        let docker = super::status(&t.dirs(), &dir, "web", Runtime::Docker).await.unwrap().controls;
        assert_eq!(docker, [Control::Shutdown]);
        let vm = super::status(&t.dirs(), &dir, "web", Runtime::Vm).await.unwrap().controls;
        assert_eq!(vm, [Control::Pause, Control::Shutdown, Control::Provision, Control::Repair]);
        assert_eq!(super::status(&t.dirs(), &dir, "web", Runtime::Cloud).await.unwrap().controls, [] as [Control; 0]);
    }
}
