//! The labs' commands: launching a lab, its runtime operations (stop, park, resume, provision,
//! status, checks, tools, attack box), and the operations in flight. Each is a thin adapter: it
//! validates what the UI passes (ids, images, boxes, machine names), turns channels into log
//! sinks, and calls the runtime domain (`crate::runtime`).

use std::path::{Path, PathBuf};

use tauri::ipc::Channel;

use super::lines;
use crate::AppHandle;
use crate::app::deploy_worker::{self, Job, Op};
use crate::app::operations::{self, ActiveOperation};
use crate::error::{Error, Result};
use crate::labs::{self, LaunchRequest};
use crate::platform::AppDirs;
use crate::runtime::attack_machine::AttackStatus;
use crate::runtime::paths::{installed_lab_dir, lab_dir, local_runtime};
use crate::runtime::providers::Provider;
use crate::runtime::{LabStatus, Park, Runtime, attack_machine, cloud_services, discover, docker, lab, start_place, status, terminal_launch};

/// Starts a lab, the deploy's log sent on `logs` (see [`labs::prepare_launch`]).
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "the webview sends each of the request's fields as an argument")]
pub async fn lab_launch(
    app: AppHandle,
    lab_id: String,
    provider: Option<Provider>,
    host: Option<String>,
    default_ports: Option<bool>,
    offline: Option<bool>,
    per_machine_vm: Option<bool>,
    docker_engine: Option<String>,
    logs: Channel<String>,
) -> Result<()> {
    let request = LaunchRequest {
        lab_id,
        provider,
        host,
        default_ports: default_ports.unwrap_or(false),
        offline: offline.unwrap_or(false),
        per_machine_vm: per_machine_vm.unwrap_or(false),
        docker_engine,
    };
    // The deploy runs in a detached worker process, so quitting (or crashing) the app never cuts a
    // vagrant/docker/terraform run short; this command only follows the worker's log.
    let launch = labs::prepare_launch(&AppDirs::from(&app), request, &lines(logs.clone())).await?;
    deploy_worker::run_job(&app, Job::from(launch), logs).await
}

/// Imports and starts a lab from a local directory `path` (an unpublished lab), the deploy's log
/// sent on `logs` (see [`labs::local_id`]).
#[tauri::command]
pub async fn lab_launch_local(app: AppHandle, path: String, logs: Channel<String>) -> Result<()> {
    let src = PathBuf::from(path);
    let lab_id = labs::local_id(&src)?;
    // Detached worker, like every lab operation; this command only follows its log.
    let job = Job::on_lab(&lab_id, Op::LaunchLocal { src: src.to_string_lossy().into_owned() });
    deploy_worker::run_job(&app, job, logs).await
}

/// The labs imported from a folder installed here, each with what its own `.ctf/` folder says
/// of it (see [`labs::imported`]): their lab page's, since no catalogue lists them.
#[tauri::command]
pub async fn imported_labs(app: AppHandle) -> Result<Vec<labs::imported::Imported>> {
    labs::imported::list(&AppDirs::from(&app))
}

/// The labs a deploy worker is running an operation on.
#[tauri::command]
pub async fn deploying_labs(app: AppHandle) -> Vec<String> {
    operations::deploying_labs(AppDirs::from(&app)).await
}

/// Every lab operation in flight, here or in a deploy worker.
#[tauri::command]
pub async fn active_operations(app: AppHandle) -> Vec<ActiveOperation> {
    operations::active_operations(AppDirs::from(&app)).await
}

/// The log of lab `id`'s last deploy.
#[tauri::command]
pub async fn lab_deploy_log(app: AppHandle, id: String) -> Result<String> {
    operations::lab_deploy_log(AppDirs::from(&app), id).await
}

fn check_image(image: &str) -> Result<()> {
    if crate::platform::exec::valid_image(image) { Ok(()) } else { Err(Error::Invalid(format!("invalid attack-box image `{image}`"))) }
}

/// A machine name as Vagrant and Isoloom spell them: plain characters, dots allowed.
fn valid_machine_name(m: &str) -> bool {
    m.len() <= 64 && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Stops lab `id` wherever it runs and removes it, its log lines sent on `logs` (see [`stop_lab`]).
#[tauri::command]
pub async fn lab_stop(app: AppHandle, id: String, runtime: Runtime, logs: Channel<String>) -> Result<()> {
    stop_lab(&AppDirs::from(&app), &id, runtime, lines(logs)).await
}

/// Stops lab `id` wherever it runs, the deploy still running in its worker ended first (see
/// [`labs::stop_lab`]).
pub(crate) async fn stop_lab(dirs: &AppDirs, id: &str, runtime: Runtime, log: impl FnMut(String)) -> Result<()> {
    labs::stop_lab(dirs, id, runtime, deploy_worker::end_workers(dirs, id), log).await
}

/// Pauses (state saved) or shuts down (powered off) a lab on this machine, keeping its machines
/// so it resumes as it was. Runs detached (a worker), like every lab operation.
#[tauri::command]
pub async fn lab_park(app: AppHandle, id: String, runtime: Runtime, mode: Park, logs: Channel<String>) -> Result<()> {
    lab_dir(&AppDirs::from(&app), &id)?;
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::Park { runtime, mode }), logs).await
}

/// Runs a VM lab's provisioners again, on one machine (`machine`) or all of them.
#[tauri::command]
pub async fn lab_provision(app: AppHandle, id: String, runtime: Runtime, machine: Option<String>, logs: Channel<String>) -> Result<()> {
    lab_dir(&AppDirs::from(&app), &id)?;
    if let Some(m) = &machine
        && !valid_machine_name(m)
    {
        return Err(Error::Invalid(format!("invalid machine name `{m}`")));
    }
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::Provision { runtime, machine }), logs).await
}

/// Brings a parked lab back (or the machines of a lab that went down).
#[tauri::command]
pub async fn lab_resume(app: AppHandle, id: String, runtime: Runtime, logs: Channel<String>) -> Result<()> {
    lab_dir(&AppDirs::from(&app), &id)?;
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::Resume { runtime }), logs).await
}

/// What lab `id` looks like right now, wherever it runs; not running when its last launch or
/// resume failed in its worker, whatever machines it left up, nor when it isn't installed (never
/// started here, or a launch that failed before its install). That last one was an error: the
/// attack box started "with the lab" when a launch ends read it, after a launch that failed in
/// its download, as a failure of its own ("lab … is not installed").
#[tauri::command]
pub async fn lab_status(app: AppHandle, id: String, runtime: Runtime) -> Result<LabStatus> {
    let dirs = AppDirs::from(&app);
    let Some(dir) = installed_lab_dir(&dirs, &id)? else { return Ok(LabStatus::default()) };
    let failed = deploy_worker::last_deploy_failed(&dirs, &id);
    Ok(status::status(&dirs, &dir, &id, runtime).await?.unless_launch_failed(failed))
}

/// Before a cloud-services lab starts: its cloud, its cost estimate, what it asks for at launch
/// and what it exposes, from its spec at the catalogue's pinned commit. None for other labs.
#[tauri::command]
pub async fn lab_cloud_preview(app: AppHandle, id: String, source: LabSource) -> Result<Option<cloud_services::Preview>> {
    cloud_services::preview_at(&AppDirs::from(&app), &id, source.checked()).await
}

/// A lab's minimum-to-run system requirements (CPU, memory, disk), as a lightest-to-heaviest
/// range over the ways it runs, from its spec at the catalogue's pinned commit. None for a lab
/// with no machines of its own (cloud services).
#[tauri::command]
pub async fn lab_requirements(app: AppHandle, id: String, source: LabSource) -> Result<Option<lab::Requirements>> {
    let spec = lab::spec_at(&AppDirs::from(&app), &id, source.checked()).await?;
    Ok(lab::requirements(&spec))
}

/// A lab's source: its repository at the catalogue's pinned commit. Every command that reads a
/// lab's spec before it is installed takes it.
#[derive(serde::Deserialize)]
pub struct LabSource {
    repository: String,
    commit: String,
}

impl LabSource {
    /// The source, with the catalogue's rule for which repositories may be fetched.
    fn checked(&self) -> lab::Source<'_> {
        labs::source(&self.repository, &self.commit)
    }
}

/// Where lab `id`, declared `runtime`, starts when no place is picked
/// ([`start_place::default_place`]: where it last ran, else a ready hypervisor here for a VM lab,
/// `preferred` (the player's Settings choice) first, else the default server), from its spec at
/// its `source`.
#[tauri::command]
pub async fn lab_default_place(
    app: AppHandle,
    id: String,
    runtime: Runtime,
    source: LabSource,
    preferred: Option<Provider>,
) -> Result<start_place::StartPlace> {
    start_place::default_place(&AppDirs::from(&app), &id, runtime, source.checked(), preferred).await
}

/// Every way lab `id`, declared `runtime`, can start ([`lab::run_options`]): per form, the
/// hypervisors, servers and clouds its spec at the catalogue's pinned commit runs on, each with
/// its launch path's badge, and where Isoloom refuses it. What the launch picker offers.
#[tauri::command]
pub async fn lab_run_options(app: AppHandle, id: String, runtime: Runtime, source: LabSource) -> Result<lab::RunOptions> {
    let lab = lab::lab_at(&AppDirs::from(&app), &id, source.checked()).await?;
    Ok(lab::run_options(&lab, runtime))
}

/// The observers a lab, declared `runtime`, puts beside itself (`tools:` in its spec), at their
/// addresses in the form it was started in ([`local_runtime`]): container addresses for a
/// container lab wherever its Compose file runs, VM addresses for a lab run as VMs (a container
/// lab run as one VM per machine included); none beside cloud services.
#[tauri::command]
pub async fn lab_tools(app: AppHandle, id: String, runtime: Runtime) -> Result<Vec<lab::Observer>> {
    let dir = lab_dir(&AppDirs::from(&app), &id)?;
    match local_runtime(&dir, runtime) {
        Runtime::Cloud => Ok(Vec::new()),
        form => lab::tools(&dir, form == Runtime::Docker),
    }
}

/// The ids of labs with something up on this machine right now, found by asking Docker and each
/// hypervisor directly (`vmrun`, `VBoxManage`, `prlctl`, `utmctl`, `virsh`, Hyper-V). One scan for
/// every lab, unlike `lab_status`: the UI reads it only to pick which labs to read the status of
/// again (one started from the website, or after a crash or restart), never as whether one runs.
#[tauri::command]
pub async fn running_labs(app: AppHandle) -> Vec<String> {
    discover::running_lab_ids(&AppDirs::from(&app)).await
}

/// Runs a lab's exploitability check: does the intended exploit path still work? Lets a
/// learner who broke their box know to reset it. Through the runner, wherever the lab runs: on
/// this machine, on a server (over SSH via the lab's router), in a cloud account (over SSH to the
/// lab), or on an ESXi host (from its controller).
#[tauri::command]
pub async fn lab_check(app: AppHandle, id: String, runtime: Runtime) -> Result<docker::Check> {
    let dirs = AppDirs::from(&app);
    let dir = lab_dir(&dirs, &id)?;
    match crate::runtime::host::locate(&dirs, &dir, &id, runtime)? {
        Some(host) => host.check().await,
        None => Ok(docker::Check::default()),
    }
}

/// Where lab `id`'s attack box goes ([`attack_machine::locate`]), or an error when the launcher
/// reaches no run of the lab.
fn with_attack_box(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<attack_machine::Placed> {
    attack_machine::locate(dirs, dir, id, runtime)?
        .ok_or_else(|| Error::Invalid("This lab has no attack box the launcher can reach. Work from what the lab gives you.".into()))
}

/// The command the runner builds to open a shell into lab `id`'s attack box (the in-app terminal
/// and the system terminal both run it).
pub(crate) async fn box_shell_command(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<isothrum_runner::ShellCommand> {
    let dir = lab_dir(dirs, id)?;
    with_attack_box(dirs, &dir, id, runtime)?.shell_command().await
}

/// The attack box's status for the lab detail view: the machine the launcher added to the lab,
/// read by its name from the run's status, the lab networks the learner may pick for it and the
/// one it joins unpicked. Unreachable (down, with nothing to pick) where the launcher reaches no
/// run. Boxed: the runner's
/// status future is large, and awaiting it inline would make every caller's future large too.
#[tauri::command]
pub async fn attack_box_status(app: AppHandle, id: String, runtime: Runtime) -> Result<AttackStatus> {
    Box::pin(async move {
        let dirs = AppDirs::from(&app);
        let dir = lab_dir(&dirs, &id)?;
        match attack_machine::locate(&dirs, &dir, &id, runtime)? {
            Some(placed) => placed.status().await,
            None => Ok(AttackStatus::default()),
        }
    })
    .await
}

/// The learner's pick for an attach: which lab network to join (none: the seat the lab declares,
/// else its first network), with which image for a container box (none: the launcher's default,
/// [`attack_machine::default_image`]; a VM box is Kali whatever is picked). One argument so the
/// command stays within Tauri's argument count.
#[derive(serde::Deserialize)]
pub struct AttachPick {
    network: Option<String>,
    image: Option<String>,
}

/// Adds the attack box to the running lab as a machine of it, on the network the learner picked
/// (else the seat the lab declares, else its first network), then brings the run up to date; a
/// container box pulls its image on the first add.
#[tauri::command]
pub async fn attack_box_attach(app: AppHandle, id: String, runtime: Runtime, pick: AttachPick, logs: Channel<String>) -> Result<()> {
    Box::pin(async move {
        let AttachPick { network, image } = pick;
        let dirs = AppDirs::from(&app);
        let dir = lab_dir(&dirs, &id)?;
        if let Some(image) = &image {
            check_image(image)?;
        }
        // Held for the whole attach, like every lab operation: a pause or a stop from another
        // window or the agent waits, rather than halting the VMs the attach is working in.
        let _lock = crate::runtime::lock_lab(&dirs, &id).await?;
        let run = with_attack_box(&dirs, &dir, &id, runtime)?;
        let mut log = lines(logs);
        let image = image.unwrap_or_else(|| attack_machine::default_image().to_owned());
        let attach = run.add(&dirs, &image, network.as_deref(), &mut log);
        // Only an image pulled on this machine is the app's to account for.
        if !run.pulls_image_here() {
            return Box::pin(attach).await;
        }
        // Boxed: the runner's attach future is large, and tracking it adds to it.
        Box::pin(crate::machine::storage::owned::tracked(&app, std::slice::from_ref(&image), &[], attach)).await
    })
    .await
}

/// Takes the attack box back out of the running lab, leaving the rest of the lab up.
#[tauri::command]
pub async fn attack_box_detach(app: AppHandle, id: String, runtime: Runtime, logs: Channel<String>) -> Result<()> {
    Box::pin(async move {
        let dirs = AppDirs::from(&app);
        let dir = lab_dir(&dirs, &id)?;
        let _lock = crate::runtime::lock_lab(&dirs, &id).await?;
        with_attack_box(&dirs, &dir, &id, runtime)?.remove(&dirs, &mut lines(logs)).await
    })
    .await
}

/// Opens the attack box shell of a running lab in the system terminal, on the command the runner
/// builds (it alone knows the Compose project and files, the Vagrant define).
#[tauri::command]
pub async fn lab_attack_shell(app: AppHandle, id: String, runtime: Runtime) -> Result<()> {
    let dirs = AppDirs::from(&app);
    let shell = box_shell_command(&dirs, &id, runtime).await?;
    terminal_launch::open_terminal_async(shell).await
}

#[cfg(test)]
mod tests {
    use super::{AttachPick, LabSource, check_image, lines, valid_machine_name};
    use super::{
        attack_box_attach, attack_box_detach, attack_box_status, imported_labs, lab_attack_shell, lab_check, lab_cloud_preview, lab_default_place, lab_launch,
        lab_launch_local, lab_park, lab_provision, lab_resume, lab_run_options, lab_status, lab_stop, lab_tools, running_labs,
    };
    use crate::platform::exec::fake::Tools;
    use crate::runtime::{Park, Runtime, providers::Provider, server};
    use crate::testing::test_app::TestApp;
    use tauri::ipc::{Channel, InvokeResponseBody};

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";

    /// Starts an installed lab on Docker the way a launch does, minus the worker and the token.
    async fn start_docker(t: &TestApp, id: &str, logs: Channel<String>) -> crate::error::Result<()> {
        let dirs = t.dirs();
        let dir = super::lab_dir(&dirs, id)?;
        let lab = crate::runtime::LabRef { dir: &dir, id, runtime: Runtime::Docker };
        crate::runtime::start(&dirs, lab, crate::runtime::Destination::default(), &[], super::lines(logs)).await
    }

    fn sink() -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
    }

    #[tokio::test]
    async fn imported_labs_are_the_local_ones_with_what_their_ctf_folder_defines() {
        let t = TestApp::new();
        t.lab("66d71cbf-9b4e-4a78-b3cf-8208227a96b2", &[("isoloom.yml", SPEC)]);
        t.lab("local-t", &[("isoloom.yml", SPEC), (".ctf/metadata.json", r#"{ "title": "Supplier portal API" }"#), (".ctf/objectives.json", "[]")]);
        let labs = imported_labs(t.handle().clone()).await.unwrap();
        let [lab] = labs.as_slice() else { panic!("{labs:?}") };
        assert_eq!((lab.id.as_str(), lab.name.as_str()), ("local-t", "t"));
        assert_eq!(lab.metadata.as_ref().map(|m| m.title.as_str()), Some("Supplier portal API"));
        assert_eq!(lab.objectives, None);
        assert_eq!(lab.problems.iter().map(|p| (p.file.as_str(), p.error.as_str())).collect::<Vec<_>>(), [("objectives.json", "not a JSON object")]);
    }

    #[tokio::test]
    async fn running_labs_lists_nothing_when_no_lab_is_running() {
        // An installed but never-started lab is not running; the quit guard and the tray read
        // this. A running lab's listing is covered by the integration tests.
        let t = TestApp::new();
        t.lab("web", &[("isoloom.yml", SPEC)]);
        let _tools = Tools::rules(&[]);
        assert_eq!(running_labs(t.handle().clone()).await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_observers_are_at_their_addresses_in_the_form_the_lab_was_started_in() {
        let t = TestApp::new();
        let spec = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n    os: debian@12\ntools:\n  shell: {}\n";
        let dir = t.lab("obs", &[("isoloom.yml", spec)]);
        // A second instance of the lab: its Docker blocks are its own, unlike its VMs'.
        std::fs::write(dir.join(crate::runtime::lab::INSTANCE_MARKER), "2").unwrap();
        let tools = |runtime| lab_tools(t.handle().clone(), "obs".into(), runtime);
        let docker = tools(Runtime::Docker).await.unwrap();
        let vms = tools(Runtime::Vm).await.unwrap();
        assert_ne!(docker[0].addresses[0].ip, vms[0].addresses[0].ip, "a container and a VM address differ");
        // A container lab run as one VM per machine: its observers are at their VM addresses.
        crate::runtime::paths::mark_per_machine_vm(&dir, true).unwrap();
        assert_eq!(tools(Runtime::Docker).await.unwrap()[0].addresses[0].ip, vms[0].addresses[0].ip);
        // Beside cloud services, none.
        assert!(tools(Runtime::Cloud).await.unwrap().is_empty());
    }

    /// A lab's source as the webview sends it.
    fn source(repository: &str, commit: &str) -> LabSource {
        LabSource { repository: repository.into(), commit: commit.into() }
    }

    #[tokio::test]
    async fn the_preview_command_reads_the_installed_copy_and_refuses_a_source_the_catalogue_doesnt_allow() {
        let t = TestApp::new();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let spec = "version: 1\nname: goat\ncloud:\n  provider: aws\n  terraform: terraform\n  hourly_usd: 0.02\n";
        t.lab("goat", &[("isoloom.yml", spec), ("terraform/main.tf", ""), (crate::runtime::lab::COMMIT_MARKER, commit)]);
        let preview = lab_cloud_preview(t.handle().clone(), "goat".into(), source("CyberCTF/goat", commit)).await.unwrap();
        assert_eq!(preview.map(|p| p.provider).as_deref(), Some("aws"));
        // Refused before anything is downloaded: a bad lab id, another owner, a short commit.
        let refuse = |id: &str, repo: &str, commit: &str| lab_cloud_preview(t.handle().clone(), id.into(), source(repo, commit));
        assert_eq!(refuse("../x", "CyberCTF/goat", commit).await.unwrap_err().to_string(), "invalid lab id `../x`");
        assert_eq!(refuse("goat", "evil/goat", commit).await.unwrap_err().to_string(), format!("refusing to download evil/goat@{commit}"));
        assert_eq!(refuse("goat", "CyberCTF/goat", "main").await.unwrap_err().to_string(), "refusing to download CyberCTF/goat@main");
    }

    #[tokio::test]
    async fn a_container_lab_has_no_default_server() {
        let t = TestApp::new();
        let pve = t.host("pve", "proxmox", serde_json::json!({}));
        crate::runtime::server::store::update(&t.dirs(), |s| {
            s.default = Some(pve);
            Ok(())
        })
        .unwrap();
        let sha = "0123456789abcdef0123456789abcdef01234567".to_string();
        // Never run: on Docker here, its spec not even read.
        let place = lab_default_place(t.handle().clone(), "web".into(), Runtime::Docker, source("CyberCTF/web", &sha), None).await.unwrap();
        assert_eq!(place, crate::runtime::start_place::StartPlace::default());
    }

    #[tokio::test]
    async fn the_picker_is_offered_the_servers_the_labs_targets_run_on() {
        let t = TestApp::new();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let vms = "version: 1\nname: ad\nnetworks: { lan: { cidr: 10.20.0.0/24 } }\nmachines:\n  dc: { os: windows-server@2022, networks: { lan: 10 } }\ntargets: [vagrant]\n";
        let dir = t.lab("ad", &[("isoloom.yml", vms)]);
        std::fs::write(dir.join(crate::runtime::lab::COMMIT_MARKER), sha).unwrap();
        // The installed copy at that commit is read: no download.
        let options = lab_run_options(t.handle().clone(), "ad".into(), Runtime::Vm, source("CyberCTF/ad", sha)).await.unwrap();
        let servers: Vec<_> = options.servers.iter().map(|p| (p.provider, p.target.as_str())).collect();
        assert_eq!(servers, [(Some(Provider::VmwareEsxi), "vagrant")], "its targets leave Proxmox out");
    }

    #[tokio::test]
    async fn a_lab_on_a_server_host_has_no_local_check() {
        let t = TestApp::new();
        let dir = t.lab("web", &[]);
        server::mark_lab(&dir, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
        let _tools = Tools::rules(&[]);
        let check = lab_check(t.handle().clone(), "web".into(), Runtime::Docker).await.unwrap();
        assert!(!check.available);
        let vm = lab_check(t.handle().clone(), "web".into(), Runtime::Vm).await.unwrap();
        assert!(!vm.available);
    }

    #[test]
    fn a_cloud_lab_has_its_attack_box_in_both_forms_with_its_accounts_place() {
        use crate::runtime::paths::mark_per_machine_vm;
        let t = TestApp::new();
        let _tools = Tools::rules(&[]);
        let dir = t.lab("srv", &[("isoloom.yml", SPEC)]);
        server::mark_lab(&dir, Some(&t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true })))).unwrap();
        // The box is a machine the launcher adds to the lab, the run updated with the account's
        // place: cloud-docker (the default), a container among the lab's in its cloud VM.
        let placed = |runtime| match crate::runtime::attack_machine::locate(&t.dirs(), &dir, "srv", runtime).unwrap() {
            Some(b) => match &b.host {
                crate::runtime::host::Host::Cloud { place, .. } => (place.label.clone(), b.target, b.vm),
                _ => panic!("in the cloud account"),
            },
            None => panic!("a box"),
        };
        assert_eq!(placed(Runtime::Docker), ("aws1".into(), crate::runtime::lab::Target::CloudDocker, false));
        // One VM per machine (cloud-vm): a Kali VM among the lab's.
        mark_per_machine_vm(&dir, true).unwrap();
        assert_eq!(placed(Runtime::Docker), ("aws1".into(), crate::runtime::lab::Target::CloudVm, true));
    }

    #[tokio::test]
    async fn a_lab_that_isnt_installed_reads_as_not_running() {
        // Regression (karen): a launch whose worker died in its download left the lab not
        // installed, and the attack box started with each lab, reading its status when the launch
        // ended, told "The attack box couldn't be added with the lab: lab … is not installed".
        let t = TestApp::new();
        let tools = Tools::rules(&[]);
        let status = lab_status(t.handle().clone(), "gone".into(), Runtime::Docker).await.unwrap();
        assert!(!status.running && status.machines.is_empty() && status.parked.is_none());
        // Not a way around the id's check.
        let bad = lab_status(t.handle().clone(), "../x".into(), Runtime::Docker).await.err().unwrap().to_string();
        assert_eq!(bad, "invalid lab id `../x`");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn lab_commands_refuse_labs_that_are_not_installed_and_bad_input() {
        let t = TestApp::new();
        let app = || t.handle().clone();
        let tools = Tools::rules(&[]);
        let s = sink();
        let missing = "lab `gone` is not installed";
        assert_eq!(start_docker(&t, "gone", channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_stop(app(), "gone".into(), Runtime::Docker, channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_park(app(), "gone".into(), Runtime::Docker, Park::Pause, channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_resume(app(), "gone".into(), Runtime::Vm, channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_provision(app(), "gone".into(), Runtime::Vm, None, channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert!(lab_tools(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_check(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_attack_shell(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(attack_box_status(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(kali_attach_fails(app(), "gone", channel(s.clone())).await);
        assert!(attack_box_detach(app(), "gone".into(), Runtime::Docker, channel(s.clone())).await.is_err());
        // Installed, but a machine name or image that could reach a command line is refused.
        t.lab("ad", &[]);
        let err = lab_provision(app(), "ad".into(), Runtime::Vm, Some("a;rm".into()), channel(s.clone())).await.unwrap_err().to_string();
        assert_eq!(err, "invalid machine name `a;rm`");
        assert_eq!(
            attack_box_attach(app(), "ad".into(), Runtime::Vm, AttachPick { network: None, image: Some("-x/y z".into()) }, channel(s.clone()))
                .await
                .unwrap_err()
                .to_string(),
            "invalid attack-box image `-x/y z`"
        );
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(s.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_attack_box_is_read_and_changed_only_through_the_runner() {
        let t = TestApp::new();
        let tools = Tools::rules(&[]);
        // Installed but never started: the run has no manifest, so the box reads as down, with no
        // runtime touched (everything goes through the runner, which has nothing to report yet).
        t.lab("web", &[("isoloom.yml", SPEC)]);
        let st = attack_box_status(t.handle().clone(), "web".into(), Runtime::Docker).await.unwrap();
        assert!(st.reachable && !st.running && st.ip.is_empty());
        assert_eq!((st.networks.as_slice(), st.default_network.as_str()), (["lab".to_owned()].as_slice(), "lab"), "what it can join, from the lab's spec");
        // A lab in a cloud account goes to the runner too (its box is a machine added to the
        // lab): with no spec to place it by, its status doesn't read, an attach fails and leaves
        // the lab as published, and a detach has nothing to take out; nothing runs.
        let dir = t.lab("srv", &[]);
        server::mark_lab(&dir, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
        let s = sink();
        assert!(attack_box_status(t.handle().clone(), "srv".into(), Runtime::Docker).await.is_err());
        assert!(kali_attach_fails(t.handle().clone(), "srv", channel(s.clone())).await);
        assert!(!crate::runtime::attack_machine::added(&dir).unwrap(), "the lab is as published");
        attack_box_detach(t.handle().clone(), "srv".into(), Runtime::Docker, channel(s.clone())).await.unwrap();
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(s.lock().unwrap().is_empty());
    }

    /// Whether attaching the Kali attack box to lab `id`, a Docker lab, fails.
    async fn kali_attach_fails(app: crate::AppHandle, id: &str, logs: Channel<String>) -> bool {
        let pick = AttachPick { network: None, image: Some("kalilinux/kali-rolling".into()) };
        attack_box_attach(app, id.into(), Runtime::Docker, pick, logs).await.is_err()
    }

    /// A channel whose messages land in `sink`.
    fn channel(sink: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> Channel<String> {
        Channel::new(move |body| {
            if let InvokeResponseBody::Json(j) = body {
                sink.lock().unwrap().push(j);
            }
            Ok(())
        })
    }

    #[test]
    fn logs_stream_line_by_line_to_the_ui() {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = lines(channel(sink.clone()));
        log("Starting…".into());
        log("Done".into());
        assert_eq!(*sink.lock().unwrap(), ["\"Starting…\"", "\"Done\""]);
    }

    #[tokio::test]
    async fn attack_box_commands_refuse_bad_ids_and_images_before_touching_the_runner() {
        let t = TestApp::new();
        assert!(attack_box_status(t.handle().clone(), "../x".into(), Runtime::Docker).await.is_err());
        let sink = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        t.lab("web", &[("isoloom.yml", SPEC)]);
        assert!(
            attack_box_attach(
                t.handle().clone(),
                "a b".into(),
                Runtime::Docker,
                AttachPick { network: None, image: Some("kalilinux/kali-rolling".into()) },
                channel(sink.clone())
            )
            .await
            .is_err()
        );
        let bad_image = attack_box_attach(
            t.handle().clone(),
            "web".into(),
            Runtime::Docker,
            AttachPick { network: None, image: Some("x;y".into()) },
            channel(sink.clone()),
        )
        .await
        .unwrap_err()
        .to_string();
        assert_eq!(bad_image, "invalid attack-box image `x;y`");
        assert!(sink.lock().unwrap().is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn machine_names_outside_plain_ascii_are_refused_before_a_worker_starts() {
        let t = TestApp::new();
        t.lab("ad", &[]);
        let tools = Tools::rules(&[]);
        let s = sink();
        for bad in ["dc01é", "dc01 ", "dc01\n", "ws01/x"] {
            let err = lab_provision(t.handle().clone(), "ad".into(), Runtime::Vm, Some(bad.into()), channel(s.clone())).await.unwrap_err().to_string();
            assert_eq!(err, format!("invalid machine name `{bad}`"));
        }
        assert!(tools.calls().is_empty() && s.lock().unwrap().is_empty());
    }

    #[test]
    fn machine_names_are_plain() {
        for good in ["dc01", "ws-01", "kingslanding.sevenkingdoms", "a_b"] {
            assert!(valid_machine_name(good), "{good:?} should be accepted");
        }
        for bad in ["a b", "a;rm", "../x", "a/b", "é", &"x".repeat(65)] {
            assert!(!valid_machine_name(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn image_errors_name_what_was_wrong() {
        assert!(check_image("kalilinux/kali-rolling").is_ok());
        assert_eq!(check_image("-x").unwrap_err().to_string(), "invalid attack-box image `-x`");
    }

    #[tokio::test]
    async fn lab_launch_offline_refuses_a_lab_that_is_not_installed() {
        // An offline start runs the installed lab: with none installed it is refused here, before
        // the backend or a deploy worker is involved, so nothing streams.
        let t = TestApp::new();
        let s = sink();
        let err = lab_launch(t.handle().clone(), "invoice".into(), None, None, None, Some(true), None, None, channel(s.clone())).await;
        assert!(err.is_err(), "{err:?}");
        assert!(s.lock().unwrap().is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn lab_launch_local_refuses_a_folder_without_a_lab() {
        let t = TestApp::new();
        let dir = std::env::temp_dir().join(format!("cyberctf-notalab-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = sink();
        // No isoloom.yml: there is no local id, so the command refuses before it reaches a worker.
        let err = lab_launch_local(t.handle().clone(), dir.to_string_lossy().into_owned(), channel(s.clone())).await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.is_err(), "a folder with no isoloom.yml is not a lab");
        assert!(s.lock().unwrap().is_empty(), "nothing ran");
    }
}
