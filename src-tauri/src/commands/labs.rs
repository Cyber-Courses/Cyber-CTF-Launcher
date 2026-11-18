//! The labs' commands: launching a lab, its runtime operations (stop, park, resume, provision,
//! status, checks, tools, attack box), and the operations in flight. Each is a thin adapter: it
//! validates what the UI passes (ids, images, boxes, machine names) and asks the run service
//! (`crate::run_service`) for an operation, or the runtime domain (`crate::runtime`) for a read.
//! An operation's command answers with its id; its log and its end reach the webview as run events.

use std::path::PathBuf;

use crate::AppHandle;
use crate::app::operations;
use crate::error::{Error, Result};
use crate::labs::{self, LaunchRequest};
use crate::platform::AppDirs;
use crate::run_service::{self, AttachPick, OperationId, Request};
use crate::runtime::attack_box::AttackStatus;
use crate::runtime::paths::{installed_lab_dir, lab_dir};
use crate::runtime::placement::{self, Placement};
use crate::runtime::{LabStatus, Park, Runtime, attack_box, docker, lab, status, terminal_launch};

/// Starts a lab where `placement` says, taken as it is (see [`labs::prepare_launch`]): the
/// operation's id, its log and end as run events.
#[tauri::command]
pub async fn lab_launch(app: AppHandle, lab_id: String, placement: Placement, offline: Option<bool>) -> Result<OperationId> {
    let request = LaunchRequest { lab_id, placement, offline: offline.unwrap_or(false) };
    crate::app::runs::of(&app).start(Request::Launch(request))
}

/// Imports and starts a lab from a local directory `path` (an unpublished lab; see
/// [`labs::local_id`]): the operation's id.
#[tauri::command]
pub async fn lab_launch_local(app: AppHandle, path: String) -> Result<OperationId> {
    crate::app::runs::of(&app).start(local_request(PathBuf::from(path))?)
}

/// The request that imports and starts the lab in folder `src`, under the id it gets here
/// ([`labs::local_id`]): refused for a folder that isn't a lab.
pub(crate) fn local_request(src: PathBuf) -> Result<Request> {
    let lab_id = labs::local_id(&src)?;
    Ok(Request::LaunchLocal { lab_id, src })
}

/// Starts `request` on `service` once it passes the checks every caller of the run service makes
/// (the app's commands, the command line): an operation on an installed lab needs it installed,
/// and a machine name or an image that could reach a command line is refused. The operation's id.
pub(crate) fn start_checked(dirs: &AppDirs, service: &dyn run_service::RunService, request: Request) -> Result<OperationId> {
    if !matches!(request, Request::Launch(_) | Request::LaunchLocal { .. }) {
        lab_dir(dirs, request.lab_id())?;
    }
    match &request {
        Request::Provision { machine: Some(m), .. } if !valid_machine_name(m) => return Err(Error::Invalid(format!("invalid machine name `{m}`"))),
        Request::AttachBox { pick: AttachPick { image: Some(image), .. }, .. } => check_image(image)?,
        _ => {}
    }
    service.start(request)
}

/// Starts `request` on the app's run service, checked ([`start_checked`]).
fn start(app: &AppHandle, request: Request) -> Result<OperationId> {
    start_checked(&AppDirs::from(app), &*crate::app::runs::of(app), request)
}

/// The labs imported from a folder installed here, each with what its own `.ctf/` folder says
/// of it (see [`labs::imported`]): their lab page's, since no catalogue lists them.
#[tauri::command]
pub async fn imported_labs(app: AppHandle) -> Result<Vec<labs::imported::Imported>> {
    labs::imported::list(&AppDirs::from(&app))
}

/// Whether lab `id` is installed here: whether it has anything to remove (see [`lab_remove`]).
#[tauri::command]
pub async fn lab_installed(app: AppHandle, id: String) -> Result<bool> {
    Ok(installed_lab_dir(&AppDirs::from(&app), &id)?.is_some())
}

/// Removes lab `id` from this computer: the app's copy of it and everything the app keeps for it,
/// never the folder an imported lab came from; refused while anything of it is up or an operation
/// runs on it (see [`labs::remove`]). What couldn't be deleted is in the answer.
#[tauri::command]
pub async fn lab_remove(app: AppHandle, id: String) -> Result<labs::remove::Removal> {
    remove_in(&AppDirs::from(&app), &id).await
}

/// [`lab_remove`], in the app whose folders are `dirs`.
pub(crate) async fn remove_in(dirs: &AppDirs, id: &str) -> Result<labs::remove::Removal> {
    // Refused while the runner service runs or holds an operation of the lab's run.
    if crate::runtime::isothrum::busy_in_service(dirs).await.map_err(Error::Invalid)?.iter().any(|l| l == id) {
        return Err(Error::coded(crate::error::Code::AlreadyInProgress));
    }
    labs::remove::remove(dirs, id).await
}

/// The labs with any operation in flight, in this process or the runner service: what an update
/// install waits for (see [`operations::busy_labs`]).
#[tauri::command]
pub async fn busy_labs(app: AppHandle) -> std::result::Result<Vec<String>, String> {
    operations::busy_labs(AppDirs::from(&app)).await
}

/// The run events after cursor `after` (every one kept, for none): what a window that (re)loads
/// replays before it follows the live ones.
#[tauri::command]
pub async fn run_events(app: AppHandle, after: Option<u64>) -> Vec<run_service::Cursored> {
    crate::app::runs::of(&app).events().since(after)
}

fn check_image(image: &str) -> Result<()> {
    if crate::platform::exec::valid_image(image) { Ok(()) } else { Err(Error::Invalid(format!("invalid attack-box image `{image}`"))) }
}

/// The longest machine name Isoloom accepts (a DNS label's length).
const MACHINE_NAME_MAX: usize = 64;

/// A machine name as Vagrant and Isoloom spell them: plain characters, dots allowed, not empty.
fn valid_machine_name(m: &str) -> bool {
    !m.is_empty() && m.len() <= MACHINE_NAME_MAX && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Stops lab `id` wherever it runs and removes it, what still runs on it ended first (see
/// [`labs::stop_lab_retiring_colocation`]): the operation's id.
#[tauri::command]
pub async fn lab_stop(app: AppHandle, id: String, runtime: Runtime) -> Result<OperationId> {
    start(&app, Request::Stop { lab_id: id, runtime })
}

/// Pauses (state saved) or shuts down (powered off) a lab on this machine, keeping its machines
/// so it resumes as it was: the operation's id.
#[tauri::command]
pub async fn lab_park(app: AppHandle, id: String, runtime: Runtime, mode: Park) -> Result<OperationId> {
    start(&app, Request::Park { lab_id: id, runtime, mode })
}

/// Runs a VM lab's provisioners again, on one machine (`machine`) or all of them: the operation's
/// id.
#[tauri::command]
pub async fn lab_provision(app: AppHandle, id: String, runtime: Runtime, machine: Option<String>) -> Result<OperationId> {
    start(&app, Request::Provision { lab_id: id, runtime, machine })
}

/// Brings a parked lab back (or the machines of a lab that went down): the operation's id.
#[tauri::command]
pub async fn lab_resume(app: AppHandle, id: String, runtime: Runtime) -> Result<OperationId> {
    start(&app, Request::Resume { lab_id: id, runtime })
}

/// What lab `id` looks like right now, wherever it runs; not running when its last launch or
/// resume failed in the runner service, whatever machines it left up, nor when it isn't installed (never
/// started here, or a launch that failed before its install). That last one was an error: the
/// attack box started "with the lab" when a launch ends read it, after a launch that failed in
/// its download, as a failure of its own ("lab … is not installed").
#[tauri::command]
pub async fn lab_status(app: AppHandle, id: String, runtime: Runtime) -> Result<LabStatus> {
    status_in(&AppDirs::from(&app), &id, runtime).await
}

/// [`lab_status`], in the app whose folders are `dirs`.
pub(crate) async fn status_in(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<LabStatus> {
    let Some(dir) = installed_lab_dir(dirs, id)? else { return Ok(LabStatus::default()) };
    let failed = crate::runtime::isothrum::last_run_failed(dirs, id).await;
    Ok(status::status(dirs, &dir, id, runtime).await?.unless_launch_failed(failed))
}

/// Watches `labs` (each with its catalogue runtime), `open` among them looked at closely (a lab
/// page shows it): their statuses, and what the scan of this machine sees up, come as run events
/// from now on (see [`run_service::RunService::watch`]). Replaces what this app watched.
#[tauri::command]
pub async fn labs_watch(app: AppHandle, labs: Vec<WatchedLab>, open: Vec<String>) -> Result<()> {
    for lab in &labs {
        crate::runtime::validate_id(&lab.id)?;
    }
    crate::app::runs::of(&app).watch(labs.into_iter().map(|l| (l.id, l.runtime)).collect(), open);
    Ok(())
}

/// A lab to watch: its id and its catalogue runtime.
#[derive(serde::Deserialize)]
pub struct WatchedLab {
    /// The lab's id.
    id: String,
    /// Its catalogue runtime.
    runtime: Runtime,
}

/// Before a cloud-services lab starts: its cloud, its cost estimate, what it asks for at launch
/// and what it exposes, from its spec at the catalogue's pinned commit. None for other labs.
#[tauri::command]
#[cfg(feature = "cloud-services")]
pub async fn lab_cloud_preview(app: AppHandle, id: String, source: LabSource) -> Result<Option<crate::runtime::Preview>> {
    crate::runtime::cloud_services::preview_at(&AppDirs::from(&app), &id, source.checked()).await
}

/// [`lab_cloud_preview`] in a build without the `cloud-services` feature: none, as for a lab of
/// another kind.
#[tauri::command]
#[cfg(not(feature = "cloud-services"))]
pub async fn lab_cloud_preview(app: AppHandle, id: String, source: LabSource) -> Result<Option<crate::runtime::Preview>> {
    let _ = (app, source);
    crate::runtime::validate_id(&id)?;
    Ok(None)
}

/// Whether this build runs labs of their own cloud services (the `cloud-services` feature): the
/// webview offers such labs, and the filter for them, only when it does.
#[tauri::command]
pub fn cloud_services_supported() -> bool {
    cfg!(feature = "cloud-services")
}

/// A lab's minimum-to-run system requirements (CPU, memory, disk), as a lightest-to-heaviest
/// range over the ways it runs, from its spec at the catalogue's pinned commit. None for a lab
/// with no machines of its own (cloud services).
#[tauri::command]
pub async fn lab_requirements(app: AppHandle, id: String, source: LabSource) -> Result<Option<lab::Requirements>> {
    // A cloud-services lab (its own lab file) has no machines of its own here.
    #[cfg(feature = "cloud-services")]
    if crate::runtime::cloud_services::preview_at(&AppDirs::from(&app), &id, source.checked()).await?.is_some() {
        return Ok(None);
    }
    let lab = lab::lab_at(&AppDirs::from(&app), &id, source.checked()).await?;
    Ok(lab::requirements(&lab))
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

/// Where lab `id`, declared `runtime`, runs for the player's `choice` ([`placement::choose`]:
/// what Isoloom allows from its spec at `source`, what is ready here, then the choice, else the
/// saved preference; none: the catalogue's source): the placement the webview then launches
/// with, as it is.
#[tauri::command]
pub async fn lab_placement(app: AppHandle, id: String, runtime: Runtime, source: Option<LabSource>, choice: placement::Choice) -> Result<Placement> {
    let dirs = AppDirs::from(&app);
    let Some(source) = source else {
        // A lab whose source the window doesn't know: the catalogue's.
        let listed = labs::catalogue_source(&dirs, &id, false).await?;
        return placement::for_lab(&dirs, &id, runtime, listed.source(), &choice).await;
    };
    placement::for_lab(&dirs, &id, runtime, source.checked(), &choice).await
}

/// Every way lab `id`, declared `runtime`, can start ([`lab::run_options`]): per form, the
/// hypervisors, servers and clouds its spec at the catalogue's pinned commit runs on, each with
/// its launch path, and where Isoloom refuses it. What the launch picker offers.
#[tauri::command]
pub async fn lab_run_options(app: AppHandle, id: String, runtime: Runtime, source: LabSource) -> Result<lab::RunOptions> {
    #[cfg(feature = "cloud-services")]
    if runtime == Runtime::Cloud {
        return crate::runtime::cloud_services::run_options_at(&AppDirs::from(&app), &id, source.checked()).await;
    }
    let lab = lab::lab_at(&AppDirs::from(&app), &id, source.checked()).await?;
    Ok(lab::run_options(&lab, runtime))
}

/// The observers a lab, declared `runtime`, puts beside itself (`tools:` in its spec), at their
/// addresses in the form its run was started in (`Recorded::form`): container addresses for a
/// container lab wherever its Compose file runs, VM addresses for a lab run as VMs (a container
/// lab run as one VM per machine included); none beside cloud services.
#[tauri::command]
pub async fn lab_tools(app: AppHandle, id: String, runtime: Runtime) -> Result<Vec<lab::Observer>> {
    tools_in(&AppDirs::from(&app), &id, runtime).await
}

/// [`lab_tools`], in the app whose folders are `dirs`.
pub(crate) async fn tools_in(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<Vec<lab::Observer>> {
    let dir = lab_dir(dirs, id)?;
    lab::tools(&dir, crate::runtime::recorded::Recorded::of(dirs, id).await?.form(runtime))
}

/// Runs a lab's exploitability check: does the intended exploit path still work? Lets a
/// learner who broke their box know to reset it. Through the runner, wherever the lab runs: on
/// this machine, on a server (over SSH via the lab's router), in a cloud account (over SSH to the
/// lab), or on an ESXi host (from its controller).
#[tauri::command]
pub async fn lab_check(app: AppHandle, id: String, runtime: Runtime) -> Result<docker::Check> {
    check_in(&AppDirs::from(&app), &id, runtime).await
}

/// [`lab_check`], in the app whose folders are `dirs`.
pub(crate) async fn check_in(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<docker::Check> {
    let dir = lab_dir(dirs, id)?;
    match crate::runtime::host::locate(dirs, &dir, id, runtime).await? {
        Some(host) => host.check().await,
        None => Ok(docker::Check::default()),
    }
}

/// The command the runner builds to open a shell into lab `id`'s attack box (the in-app terminal
/// and the system terminal both run it).
pub(crate) async fn box_shell_command(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<isothrum_runner::ShellCommand> {
    let dir = lab_dir(dirs, id)?;
    attack_box::reachable(dirs, &dir, id, runtime).await?.shell_command().await
}

/// The attack box's status for the lab detail view: the machine the launcher added to the lab,
/// read by its name from the run's status, the lab networks the learner may pick for it and the
/// one it joins unpicked. Unreachable (down, with nothing to pick) where the launcher reaches no
/// run. Boxed: the runner's
/// status future is large, and awaiting it inline would make every caller's future large too.
#[tauri::command]
pub async fn attack_box_status(app: AppHandle, id: String, runtime: Runtime) -> Result<AttackStatus> {
    box_status_in(&AppDirs::from(&app), &id, runtime).await
}

/// [`attack_box_status`], in the app whose folders are `dirs`.
pub(crate) async fn box_status_in(dirs: &AppDirs, id: &str, runtime: Runtime) -> Result<AttackStatus> {
    Box::pin(async move {
        let dir = lab_dir(dirs, id)?;
        match attack_box::locate(dirs, &dir, id, runtime).await? {
            Some(placed) => placed.status().await,
            None => Ok(AttackStatus::default()),
        }
    })
    .await
}

/// Adds the attack box to the running lab as a machine of it, on the network the learner picked
/// (else the seat the lab declares, else its first network), then brings the run up to date: the
/// operation's id. A container box pulls its image on the first add.
#[tauri::command]
pub async fn attack_box_attach(app: AppHandle, id: String, runtime: Runtime, pick: AttachPick) -> Result<OperationId> {
    start(&app, Request::AttachBox { lab_id: id, runtime, pick })
}

/// Takes the attack box back out of the running lab, leaving the rest of the lab up: the
/// operation's id.
#[tauri::command]
pub async fn attack_box_detach(app: AppHandle, id: String, runtime: Runtime) -> Result<OperationId> {
    start(&app, Request::DetachBox { lab_id: id, runtime })
}

/// How the player signs in to lab `id`'s attack box where it asks for a password (its screen's
/// login, `su`, SSH): its account and the password the launcher keeps for it; none when the lab
/// has no box, or the launcher keeps no password for it. The lab page's Credentials row.
#[tauri::command]
pub async fn attack_box_sign_in(app: AppHandle, id: String) -> Result<Option<attack_box::SignIn>> {
    sign_in_in(&AppDirs::from(&app), &id)
}

/// [`attack_box_sign_in`], in the app whose folders are `dirs`.
pub(crate) fn sign_in_in(dirs: &AppDirs, id: &str) -> Result<Option<attack_box::SignIn>> {
    attack_box::sign_in(&lab_dir(dirs, id)?, None)
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
    use super::{AttachPick, LabSource, check_image, valid_machine_name};
    use super::{
        attack_box_attach, attack_box_detach, attack_box_sign_in, attack_box_status, imported_labs, lab_attack_shell, lab_check, lab_installed, lab_launch,
        lab_launch_local, lab_park, lab_placement, lab_provision, lab_remove, lab_resume, lab_run_options, lab_status, lab_stop, lab_tools,
    };
    use crate::commands::lines;
    use crate::run_service::tests::finished;
    use crate::runtime::placement::{self, Placement};
    use crate::runtime::providers::testing::no_tools;
    use crate::runtime::{Park, Runtime, providers::Provider};
    use crate::testing::test_app::TestApp;
    use tauri::ipc::{Channel, InvokeResponseBody};

    /// Containers here, on the current context and free ports.
    const HERE: Placement = Placement::Docker { engine: None, default_ports: false };

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";

    /// Starts an installed lab on Docker the way a launch does, minus the backend and its token.
    async fn start_docker(t: &TestApp, id: &str, logs: Channel<String>) -> crate::error::Result<()> {
        let dirs = t.dirs();
        let dir = super::lab_dir(&dirs, id)?;
        let lab = crate::runtime::LabRef { dir: &dir, id, runtime: Runtime::Docker };
        crate::runtime::start(&dirs, lab, crate::runtime::Destination::at(&HERE), &[], crate::commands::lines(logs)).await
    }

    fn sink() -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
    }

    #[test]
    fn cloud_services_are_offered_only_in_a_build_with_their_feature() {
        assert_eq!(super::cloud_services_supported(), cfg!(feature = "cloud-services"));
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
    async fn an_installed_lab_is_removed_and_refused_while_an_operation_holds_it() {
        let t = TestApp::new();
        let app = || t.handle().clone();
        assert!(!lab_installed(app(), "local-t".into()).await.unwrap());
        assert_eq!(lab_remove(app(), "local-t".into()).await.unwrap_err().to_string(), "lab `local-t` is not installed");
        assert!(lab_installed(app(), "../x".into()).await.is_err());
        let dir = t.lab("local-t", &[("isoloom.yml", SPEC)]);
        assert!(lab_installed(app(), "local-t".into()).await.unwrap());
        // An operation acts on the lab (a start or a stop under way): refused, nothing deleted.
        let held = crate::run_service::LocalRunService::of(&t.dirs()).unwrap().claim("local-t").unwrap();
        assert_eq!(lab_remove(app(), "local-t".into()).await.unwrap_err().code(), crate::error::Code::AlreadyInProgress);
        assert!(dir.is_dir());
        // Released: the lab goes.
        drop(held);
        let removal = lab_remove(app(), "local-t".into()).await.unwrap();
        assert!(removal.left.is_empty(), "{removal:?}");
        assert!(!dir.exists());
        assert!(!lab_installed(app(), "local-t".into()).await.unwrap());
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
        crate::runtime::test_support::record_run(t.data(), "obs", None, "vagrant", "ready");
        assert_eq!(tools(Runtime::Docker).await.unwrap()[0].addresses[0].ip, vms[0].addresses[0].ip);
        // Beside cloud services, none.
        assert!(tools(Runtime::Cloud).await.unwrap().is_empty());
    }

    /// A lab's source as the webview sends it.
    fn source(repository: &str, commit: &str) -> LabSource {
        LabSource { repository: repository.into(), commit: commit.into() }
    }

    #[cfg(feature = "cloud-services")]
    #[tokio::test]
    async fn the_preview_command_reads_the_installed_copy_and_refuses_a_source_the_catalogue_doesnt_allow() {
        use super::lab_cloud_preview;
        let t = TestApp::new();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        t.lab("goat", &[("cloud/lab.yml", "provider: aws\nhourly_usd: 0.02\n"), ("cloud/main.tf", ""), (crate::runtime::lab::COMMIT_MARKER, commit)]);
        let preview = lab_cloud_preview(t.handle().clone(), "goat".into(), source("CyberCTF/goat", commit)).await.unwrap();
        assert_eq!(preview.map(|p| p.provider).as_deref(), Some("aws"));
        // Refused before anything is downloaded: a bad lab id, another owner, a short commit.
        let refuse = |id: &str, repo: &str, commit: &str| lab_cloud_preview(t.handle().clone(), id.into(), source(repo, commit));
        assert_eq!(refuse("../x", "CyberCTF/goat", commit).await.unwrap_err().to_string(), "invalid lab id `../x`");
        assert_eq!(refuse("goat", "evil/goat", commit).await.unwrap_err().to_string(), format!("refusing to download evil/goat@{commit}"));
        assert_eq!(refuse("goat", "CyberCTF/goat", "main").await.unwrap_err().to_string(), "refusing to download CyberCTF/goat@main");
    }

    #[tokio::test]
    async fn a_lab_is_placed_by_its_spec_the_choice_and_the_saved_preference_never_on_the_default_server_for_containers() {
        let t = TestApp::new();
        let pve = t.host("pve", "proxmox", serde_json::json!({}));
        crate::runtime::server::store::update(&t.dirs(), |s| {
            s.default = Some(pve);
            Ok(())
        })
        .unwrap();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let web = "version: 1\nname: web\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { networks: { lab: 10 }, app: { image: nginx:1.27 } }\n";
        let dir = t.lab("web", &[("isoloom.yml", web)]);
        std::fs::write(dir.join(crate::runtime::lab::COMMIT_MARKER), sha).unwrap();
        crate::settings::settings_change(&t.dirs(), &[crate::settings::Change::PortMode(Some(crate::settings::PortMode::Default))]).unwrap();
        let place = |choice| lab_placement(t.handle().clone(), "web".into(), Runtime::Docker, Some(source("CyberCTF/web", sha)), choice);
        // Nothing picked: containers here, on the saved ports; never the default server.
        assert_eq!(place(placement::Choice::default()).await.unwrap(), Placement::Docker { engine: None, default_ports: true });
        // Picked: as picked.
        let random = placement::Choice { default_ports: Some(false), ..placement::Choice::default() };
        assert_eq!(place(random).await.unwrap(), Placement::Docker { engine: None, default_ports: false });
        // A server picked for containers: refused, never moved elsewhere.
        let on_pve = placement::Choice { host: Some("pve".into()), ..placement::Choice::default() };
        assert_eq!(place(on_pve).await.unwrap_err().to_string(), crate::runtime::launch::CONTAINERS_NOT_ON_SERVERS);
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
        t.lab("web", &[]);
        crate::runtime::test_support::record_run(t.data(), "web", Some(&t.host("az1", "azure", serde_json::json!({}))), "cloud-docker/azure", "ready");
        let _tools = no_tools();
        let check = lab_check(t.handle().clone(), "web".into(), Runtime::Docker).await.unwrap();
        assert!(!check.available);
        let vm = lab_check(t.handle().clone(), "web".into(), Runtime::Vm).await.unwrap();
        assert!(!vm.available);
    }

    #[tokio::test]
    async fn a_cloud_lab_has_its_attack_box_in_both_forms_with_its_accounts_place() {
        use crate::runtime::test_support::record_run;
        let t = TestApp::new();
        let _tools = no_tools();
        let dir = t.lab("srv", &[("isoloom.yml", SPEC)]);
        let aws1 = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        record_run(t.data(), "srv", Some(&aws1), "cloud-docker/aws", "ready");
        // The box is a machine the launcher adds to the lab, the run updated with the account's
        // place: cloud-docker (the default), a container among the lab's in its cloud VM.
        let placed = async |runtime| match crate::runtime::attack_box::locate(&t.dirs(), &dir, "srv", runtime).await.unwrap() {
            Some(b) => match (b.host.run.site(), &b.host.conn) {
                (crate::runtime::isothrum::Site::Cloud(_), Some(conn)) => (conn.id.clone(), b.target, b.vm),
                _ => panic!("in the cloud account"),
            },
            None => panic!("a box"),
        };
        assert_eq!(placed(Runtime::Docker).await, ("aws1".into(), crate::runtime::lab::Target::CloudDocker, false));
        // One VM per machine (cloud-vm): a Kali VM among the lab's.
        record_run(t.data(), "srv", Some(&aws1), "cloud-vm/aws", "ready");
        assert_eq!(placed(Runtime::Docker).await, ("aws1".into(), crate::runtime::lab::Target::CloudVm, true));
    }

    #[tokio::test]
    async fn a_lab_that_isnt_installed_reads_as_not_running() {
        // Regression (Windows): a launch that died in its download left the lab not
        // installed, and the attack box started with each lab, reading its status when the launch
        // ended, told "The attack box couldn't be added with the lab: lab … is not installed".
        let t = TestApp::new();
        let tools = no_tools();
        let status = lab_status(t.handle().clone(), "gone".into(), Runtime::Docker).await.unwrap();
        assert!(!status.running && status.machines.is_empty() && status.parked.is_none());
        // Not a way around the id's check.
        let bad = lab_status(t.handle().clone(), "../x".into(), Runtime::Docker).await.err().unwrap().to_string();
        assert_eq!(bad, "invalid lab id `../x`");
        assert!(tools.lines().is_empty(), "{:?}", tools.lines());
    }

    #[tokio::test]
    async fn lab_commands_refuse_labs_that_are_not_installed_and_bad_input() {
        let t = TestApp::new();
        let app = || t.handle().clone();
        let tools = no_tools();
        let s = sink();
        let missing = "lab `gone` is not installed";
        assert_eq!(start_docker(&t, "gone", channel(s.clone())).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_stop(app(), "gone".into(), Runtime::Docker).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_park(app(), "gone".into(), Runtime::Docker, Park::Pause).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_resume(app(), "gone".into(), Runtime::Vm).await.unwrap_err().to_string(), missing);
        assert_eq!(lab_provision(app(), "gone".into(), Runtime::Vm, None).await.unwrap_err().to_string(), missing);
        assert!(lab_tools(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_check(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_attack_shell(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(attack_box_status(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(kali_attach_fails(app(), "gone").await);
        assert!(attack_box_detach(app(), "gone".into(), Runtime::Docker).await.is_err());
        // Installed, but a machine name or image that could reach a command line is refused.
        t.lab("ad", &[]);
        let err = lab_provision(app(), "ad".into(), Runtime::Vm, Some("a;rm".into())).await.unwrap_err().to_string();
        assert_eq!(err, "invalid machine name `a;rm`");
        assert_eq!(
            attack_box_attach(app(), "ad".into(), Runtime::Vm, AttachPick { network: None, image: Some("-x/y z".into()), desktop: false })
                .await
                .unwrap_err()
                .to_string(),
            "invalid attack-box image `-x/y z`"
        );
        assert!(tools.lines().is_empty(), "{:?}", tools.lines());
        assert!(s.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_attack_box_is_read_and_changed_only_through_the_runner() {
        let t = TestApp::new();
        let tools = no_tools();
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
        crate::runtime::test_support::record_run(t.data(), "srv", Some(&t.host("az1", "azure", serde_json::json!({}))), "cloud-docker/azure", "ready");
        let s = sink();
        assert!(attack_box_status(t.handle().clone(), "srv".into(), Runtime::Docker).await.is_err());
        assert!(kali_attach_fails(t.handle().clone(), "srv").await);
        assert!(!crate::runtime::attack_box::added(&dir).unwrap(), "the lab is as published");
        let detach = attack_box_detach(t.handle().clone(), "srv".into(), Runtime::Docker).await.unwrap();
        assert_eq!(finished(t.handle(), &detach).await.0, Ok(()));
        assert!(tools.lines().is_empty(), "{:?}", tools.lines());
        assert!(s.lock().unwrap().is_empty());
    }

    /// Whether attaching the Kali attack box to lab `id`, a Docker lab, fails: refused, or failed
    /// as it ran.
    async fn kali_attach_fails(app: crate::AppHandle, id: &str) -> bool {
        let pick = AttachPick { network: None, image: Some("kalilinux/kali-rolling".into()), desktop: false };
        match attack_box_attach(app.clone(), id.into(), Runtime::Docker, pick).await {
            Err(_) => true,
            Ok(operation) => finished(&app, &operation).await.0.is_err(),
        }
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
    async fn the_attack_box_sign_in_is_the_kept_password_of_the_box_and_none_without_one() {
        use crate::runtime::attack_box::{self, Cpu, Kind, PLAYER};
        let t = TestApp::new();
        let dir = t.lab("web", &[("isoloom.yml", SPEC)]);
        let sign_in = || attack_box_sign_in(t.handle().clone(), "web".into());
        assert_eq!(sign_in().await.unwrap(), None, "no box");
        attack_box::add(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, None).unwrap();
        let got = sign_in().await.unwrap().expect("the box's sign-in");
        assert_eq!(got.user, PLAYER);
        assert_eq!(got.password.len(), 23);
        attack_box::remove(&dir).unwrap();
        assert_eq!(sign_in().await.unwrap(), None, "taken out: forgotten");
        assert!(attack_box_sign_in(t.handle().clone(), "../x".into()).await.is_err(), "a bad id");
    }

    #[tokio::test]
    async fn attack_box_commands_refuse_bad_ids_and_images_before_touching_the_runner() {
        let t = TestApp::new();
        assert!(attack_box_status(t.handle().clone(), "../x".into(), Runtime::Docker).await.is_err());
        t.lab("web", &[("isoloom.yml", SPEC)]);
        assert!(
            attack_box_attach(
                t.handle().clone(),
                "a b".into(),
                Runtime::Docker,
                AttachPick { network: None, image: Some("kalilinux/kali-rolling".into()), desktop: false },
            )
            .await
            .is_err()
        );
        let bad_image =
            attack_box_attach(t.handle().clone(), "web".into(), Runtime::Docker, AttachPick { network: None, image: Some("x;y".into()), desktop: false })
                .await
                .unwrap_err()
                .to_string();
        assert_eq!(bad_image, "invalid attack-box image `x;y`");
        assert!(crate::app::runs::of(t.handle()).events().since(None).is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn machine_names_outside_plain_ascii_are_refused_before_an_operation_starts() {
        let t = TestApp::new();
        t.lab("ad", &[]);
        let tools = no_tools();
        let s = sink();
        for bad in ["", "dc01é", "dc01 ", "dc01\n", "ws01/x"] {
            let err = lab_provision(t.handle().clone(), "ad".into(), Runtime::Vm, Some(bad.into())).await.unwrap_err().to_string();
            assert_eq!(err, format!("invalid machine name `{bad}`"));
        }
        assert!(tools.lines().is_empty() && s.lock().unwrap().is_empty());
    }

    #[test]
    fn machine_names_are_plain() {
        for good in ["dc01", "ws-01", "kingslanding.sevenkingdoms", "a_b"] {
            assert!(valid_machine_name(good), "{good:?} should be accepted");
        }
        for bad in ["", "a b", "a;rm", "../x", "a/b", "é", &"x".repeat(65)] {
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
        // An offline start runs the installed lab: with none installed it fails before the backend
        // or the runner service is involved.
        let t = TestApp::new();
        let here = Placement::Docker { engine: None, default_ports: false };
        let id = lab_launch(t.handle().clone(), "invoice".into(), here, Some(true)).await.unwrap();
        let (outcome, _) = finished(t.handle(), &id).await;
        assert!(outcome.is_err(), "{outcome:?}");
    }

    #[tokio::test]
    async fn lab_launch_local_refuses_a_folder_without_a_lab() {
        let t = TestApp::new();
        let dir = std::env::temp_dir().join(format!("cyberctf-notalab-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // No isoloom.yml: there is no local id, so the command refuses before it runs anything.
        let err = lab_launch_local(t.handle().clone(), dir.to_string_lossy().into_owned()).await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.is_err(), "a folder with no isoloom.yml is not a lab");
    }
}
