//! What a lab looks like right now, wherever it runs: its machines and networks, where it runs,
//! whether it is parked, and (for a container lab on a Terraform host) the containers read from
//! that host.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::AppHandle;

use super::paths::{lab_dir, local_vm, parked, state_dir, vagrant_dirs};
use super::{AttackerAt, LabStatus, Place, Runtime, cloud_services, docker, lab, remote, server, terraform, vm};
use crate::error::Result;

pub(super) async fn status(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime) -> Result<LabStatus> {
    if runtime == Runtime::Cloud {
        return cloud_status(app, dir, id);
    }
    match server::lab_connection(app, dir)? {
        None => local_status(dir, id, runtime).await,
        Some(c) => server_status(app, dir, id, runtime, c).await,
    }
}

pub(super) async fn local_status(dir: &Path, id: &str, runtime: Runtime) -> Result<LabStatus> {
    let s = match runtime {
        // Shown like a remote lab (the attack box lives in the VM, reached over SSH).
        Runtime::Docker if let Some(p) = local_vm(dir) => in_local_vm(vm::status(&lab::vagrant_dir(dir, runtime), &[]).await?, p),
        Runtime::Docker => LabStatus { place: Some(Place::Container), ..docker::status(dir, id).await? },
        Runtime::Vm => LabStatus { place: Some(Place::LocalVm), ..vm::status(&lab::vagrant_dir(dir, runtime), &[]).await? },
        // Cloud services never run here.
        Runtime::Cloud => LabStatus::default(),
    };
    Ok(with_parked(s, parked(dir)))
}

/// A cloud-services lab's status, from its state on the account it was started on; not running
/// when its last deploy failed (what it created stays listed, to be stopped).
fn cloud_status(app: &AppHandle, dir: &Path, id: &str) -> Result<LabStatus> {
    let s = cloud_services::lab_status(app, dir, id, server::lab_connection(app, dir)?)?;
    Ok(LabStatus { running: s.running && !crate::deploy_worker::last_deploy_failed(app, id), ..s })
}

/// A container lab's VM status, as the lab's: on `provider` (a hypervisor id) on this machine.
fn in_local_vm(status: LabStatus, provider: String) -> LabStatus {
    LabStatus { host: Some(format!("{} VM on this machine", vm_label(&provider))), place: Some(Place::LocalVm), provider: Some(provider), ..status }
}

/// Parked = the launcher parked it (`parked`) and it is still down; a lab someone started again
/// by hand (or that is up for any reason) is simply running.
fn with_parked(s: LabStatus, parked: Option<super::Park>) -> LabStatus {
    LabStatus { parked: parked.filter(|_| !s.running), ..s }
}

async fn server_status(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime, c: server::Connection) -> Result<LabStatus> {
    let tf = server::terraform_target(c.provider);
    let status = match tf {
        Some(tf) => terraform::status(&state_dir(app, id, tf)?),
        None => vm::status(&lab::vagrant_dir(dir, runtime), &c.env).await?,
    };
    // Its VMs can be up after a setup step failed (Vagrant on ESXi): not running, left behind.
    let status = LabStatus { running: status.running && !crate::deploy_worker::last_deploy_failed(app, id), ..status };
    // A Docker lab on a Terraform lab host: its containers and the attack box, read from the
    // host. Never the bare VM in their place: until the host answers, no machines (the page
    // says it's reading them).
    let status = match tf {
        Some(tf) if status.running && runtime == Runtime::Docker => match remote_containers(app, id, dir, &state_dir(app, id, tf)?).await {
            Some(found) => found,
            None => LabStatus { machines: Vec::new(), networks: Vec::new(), ..status },
        },
        _ => status,
    };
    Ok(on_server(status, c.name, c.provider))
}

/// A lab's status as running on the server host `name` (a cloud account, or a server).
fn on_server(status: LabStatus, name: String, provider: super::providers::Provider) -> LabStatus {
    let place = if provider.is_cloud() { Place::Cloud } else { Place::Server };
    LabStatus { host: Some(name), place: Some(place), provider: Some(provider.id().to_string()), ..status }
}

/// A hypervisor's display name, from its Vagrant provider id.
fn vm_label(provider: &str) -> &str {
    match provider {
        "virtualbox" => "VirtualBox",
        "vmware_desktop" => "VMware",
        "parallels" => "Parallels",
        "hyperv" => "Hyper-V",
        "libvirt" => "libvirt",
        other => other,
    }
}

/// How long a remote lab's container list is reused: the lab page polls its status every few
/// seconds, and each read is an SSH round trip (through the Proxmox node, sometimes).
const REMOTE_PROBE_TTL: Duration = Duration::from_secs(10);
static REMOTE_PROBES: Mutex<Option<HashMap<String, (Instant, LabStatus)>>> = Mutex::new(None);

/// The compose project a lab runs under: the `name:` of its generated Compose file.
fn compose_project_name(dir: &Path) -> Option<String> {
    compose_name(&std::fs::read_to_string(lab::compose_file(dir)).ok()?)
}

fn compose_name(compose: &str) -> Option<String> {
    compose.lines().find_map(|l| l.strip_prefix("name:").map(|n| n.trim().trim_matches('"').trim_matches('\'').to_string())).filter(|n| !n.is_empty())
}

/// The containers on a Terraform lab host (and the attack box beside them), in the shape of a
/// local Docker lab's status. None when the host can't be read; the caller keeps the VM view.
async fn remote_containers(app: &AppHandle, id: &str, dir: &Path, state: &Path) -> Option<LabStatus> {
    let cached = cached_probe(id);
    if cached.is_some() {
        return cached;
    }
    let project = compose_project_name(dir)?;
    let target = remote::terraform_host(app, state).await.ok()??;
    let out = remote::run_on_terraform_host(&target, state, &docker::host_probe_script(&project)).await.ok()?;
    let probe = docker::HostProbe::parse(&out)?;
    let (found, attacker) = docker::status_from_host(&project, &probe);
    if found.machines.is_empty() {
        return None;
    }
    let status = LabStatus { attacker: attacker.map(|(ip, lab_network)| AttackerAt { ip, lab_network }), ..found };
    remember_probe(id, &status);
    Some(status)
}

/// The lab's remote container list read less than [`REMOTE_PROBE_TTL`] ago.
fn cached_probe(id: &str) -> Option<LabStatus> {
    REMOTE_PROBES.lock().ok().and_then(|g| g.as_ref()?.get(id).filter(|(at, _)| at.elapsed() < REMOTE_PROBE_TTL).map(|(_, s)| s.clone()))
}

fn remember_probe(id: &str, status: &LabStatus) {
    if let Ok(mut g) = REMOTE_PROBES.lock() {
        g.get_or_insert_with(HashMap::new).insert(id.to_string(), (Instant::now(), status.clone()));
    }
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

/// [`running_here`] for a lab by id; false when it isn't installed.
pub async fn lab_running_here(app: &AppHandle, id: &str) -> bool {
    match lab_dir(app, id) {
        Ok(dir) => running_here(&dir, id).await,
        Err(_) => false,
    }
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
    use super::{Place, cached_probe, compose_name, compose_project_name, in_local_vm, on_server, primary_url, remember_probe, vm_label, with_parked};
    use crate::runtime::providers::Provider;
    use crate::runtime::test_support::status_with;
    use crate::runtime::{Park, Runtime};

    #[test]
    fn a_container_lab_in_a_vm_reads_as_that_vm() {
        let s = in_local_vm(status_with(true, &["docker-vm"]), "vmware_desktop".into());
        assert_eq!(s.host.as_deref(), Some("VMware VM on this machine"));
        assert!(matches!(s.place, Some(Place::LocalVm)));
        assert_eq!(s.provider.as_deref(), Some("vmware_desktop"));
        assert!(s.running);
    }

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

    #[test]
    fn remote_probes_are_reused_for_a_while() {
        let id = "status-test-probe-cache";
        assert!(cached_probe(id).is_none());
        remember_probe(id, &status_with(true, &["web", "db"]));
        let cached = cached_probe(id).unwrap();
        assert_eq!(cached.machines.len(), 2);
        assert!(cached_probe("status-test-probe-other").is_none());
    }

    #[test]
    fn the_compose_project_comes_from_the_generated_file() {
        let dir = std::env::temp_dir().join(format!("cyberctf-status-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(compose_project_name(&dir), None);
        let file = crate::runtime::lab::compose_file(&dir);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "name: t-2\nservices: {}\n").unwrap();
        assert_eq!(compose_project_name(&dir).as_deref(), Some("t-2"));
        std::fs::remove_dir_all(dir).unwrap();
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
    fn compose_name_is_the_top_level_name_unquoted() {
        assert_eq!(compose_name("name: sqli-1\nservices:\n").as_deref(), Some("sqli-1"));
        assert_eq!(compose_name("services:\n  web: {}\nname: \"lab-2\"\n").as_deref(), Some("lab-2"));
        assert_eq!(compose_name("name: 'lab-3'\n").as_deref(), Some("lab-3"));
        // Indented `name:` keys belong to services, not the project.
        assert_eq!(compose_name("services:\n  web:\n    name: nope\n"), None);
        assert_eq!(compose_name("name: \"\"\n"), None);
    }

    #[tokio::test]
    async fn a_lab_status_is_read_where_the_lab_runs() {
        use crate::exec::fake::{Tools, ok};
        let t = crate::test_app::TestApp::new();
        let app = t.handle();
        let dir = t.lab("web", &[]);
        let _tools = Tools::rules(&[
            ("ps --all", ok(r#"{"Service":"web","State":"running","Name":"cyberctf-web-web-1"}"#)),
            ("vagrant status", ok("1,dc01,state,running\n1,dc01,provider-name,virtualbox\n")),
        ]);
        // Its containers here.
        let s = super::status(app, &dir, "web", Runtime::Docker).await.unwrap();
        assert!(s.running && matches!(s.place, Some(Place::Container)));
        assert!(super::running_here(&dir, "web").await);
        assert!(super::lab_running_here(app, "web").await);
        assert!(!super::lab_running_here(app, "gone").await);
        // Its VMs here, and a container lab in a local VM.
        let s = super::status(app, &dir, "web", Runtime::Vm).await.unwrap();
        assert!(s.running && matches!(s.place, Some(Place::LocalVm)));
        crate::runtime::paths::mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        let s = super::status(app, &dir, "web", Runtime::Docker).await.unwrap();
        assert_eq!(s.host.as_deref(), Some("VirtualBox VM on this machine"));
        // In a cloud account: its Terraform state there (none yet: not running).
        crate::runtime::server::mark_lab(&dir, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
        let s = super::status(app, &dir, "web", Runtime::Docker).await.unwrap();
        assert!(!s.running && matches!(s.place, Some(Place::Cloud)));
        assert_eq!(s.host.as_deref(), Some("Host az1"));
    }

    #[tokio::test]
    async fn a_lab_with_nothing_up_is_not_running_here() {
        use crate::exec::fake::Tools;
        let dir = std::env::temp_dir().join(format!("cyberctf-status-here-{}", rand::random::<u32>()));
        let vagrant = crate::runtime::lab::vagrant_dir(&dir, Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), "").unwrap();
        let tools = Tools::rules(&[]);
        assert!(!super::running_here(&dir, "t").await);
        assert!(tools.called("vagrant status"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vm_label_names_known_hypervisors_and_passes_others_through() {
        assert_eq!(vm_label("virtualbox"), "VirtualBox");
        assert_eq!(vm_label("vmware_desktop"), "VMware");
        assert_eq!(vm_label("hyperv"), "Hyper-V");
        assert_eq!(vm_label("qemu"), "qemu");
    }
}
