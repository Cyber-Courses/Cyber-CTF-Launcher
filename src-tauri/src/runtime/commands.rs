//! The Tauri commands over lab runtimes. They validate what the UI passes (ids, images, boxes,
//! machine names), hand long operations to the detached deploy worker, and stream logs back
//! over a channel.

use crate::AppHandle;
use tauri::ipc::Channel;

use std::path::Path;

use super::lifecycle::stop_lab;
use super::paths::{lab_dir, local_vm, runner_home};
use super::{LabStatus, Park, Runtime, cloud_services, discover, docker, isothrum, lab, server, status, terminal_launch};
use crate::deploy_worker::{self, Job, Op};
use crate::error::{Error, Result};

/// A log sink that streams each line to the UI.
fn channel_log(logs: Channel<String>) -> impl Fn(String) + Send + Sync + 'static {
    move |line: String| {
        let _ = logs.send(line);
    }
}

fn check_image(image: &str) -> Result<()> {
    if crate::platform::exec::valid_image(image) { Ok(()) } else { Err(Error::Invalid(format!("invalid attack-box image `{image}`"))) }
}

/// A machine name as Vagrant and Isoloom spell them: plain characters, dots allowed.
fn valid_machine_name(m: &str) -> bool {
    m.len() <= 64 && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

#[tauri::command]
pub async fn lab_stop(app: AppHandle, id: String, runtime: Runtime, logs: Channel<String>) -> Result<()> {
    stop_lab(&app, &id, runtime, channel_log(logs)).await
}

/// Pauses (state saved) or shuts down (powered off) a lab on this machine, keeping its machines
/// so it resumes as it was. Runs detached (a worker), like every lab operation.
#[tauri::command]
pub async fn lab_park(app: AppHandle, id: String, runtime: Runtime, mode: Park, logs: Channel<String>) -> Result<()> {
    lab_dir(&app, &id)?;
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::Park { runtime, mode }), logs).await
}

/// Runs a VM lab's provisioners again, on one machine (`machine`) or all of them.
#[tauri::command]
pub async fn lab_provision(app: AppHandle, id: String, runtime: Runtime, machine: Option<String>, logs: Channel<String>) -> Result<()> {
    lab_dir(&app, &id)?;
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
    lab_dir(&app, &id)?;
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::Resume { runtime }), logs).await
}

#[tauri::command]
pub async fn lab_status(app: AppHandle, id: String, runtime: Runtime) -> Result<LabStatus> {
    let dir = lab_dir(&app, &id)?;
    status::status(&app, &dir, &id, runtime).await
}

/// Before a cloud-services lab starts: its cloud, its cost estimate, what it asks for at launch
/// and what it exposes, from its spec at the catalogue's pinned commit. None for other labs.
#[tauri::command]
pub async fn lab_cloud_preview(app: AppHandle, id: String, repository: String, commit: String) -> Result<Option<cloud_services::Preview>> {
    cloud_services::preview_at(&app, &id, &repository, &commit).await
}

/// The observers a lab puts beside itself (`tools:` in its spec), at their addresses where it
/// runs (container addresses for a container lab, wherever its Compose file runs).
#[tauri::command]
pub async fn lab_tools(app: AppHandle, id: String, runtime: Runtime) -> Result<Vec<lab::Observer>> {
    let dir = lab_dir(&app, &id)?;
    if runtime == Runtime::Cloud {
        return Ok(Vec::new());
    }
    lab::tools(&dir, runtime == Runtime::Docker)
}

/// The ids of labs running on this machine right now, found by scanning Docker and Vagrant
/// directly. Unlike `lab_status` (one lab at a time, from its dir and Compose file), this is a
/// single infrastructure scan, so the UI recovers which labs are up even after a crash or restart,
/// and a lab whose per-lab status probe is momentarily failing still shows as running.
#[tauri::command]
pub async fn running_labs(app: AppHandle) -> Vec<String> {
    discover::running_lab_ids(&app).await
}

/// How far each launch path has been proven, from Isoloom's tier evidence: the Beta/Experimental
/// badge the launch picker shows (data-driven, no hardcoded list).
#[tauri::command]
pub fn run_tiers() -> Vec<super::RunTier> {
    super::tiers::run_tiers()
}

/// Runs a lab's exploitability check: does the intended exploit path still work? Lets a
/// learner who broke their box know to reset it. Local Docker labs, and Proxmox VM labs through
/// the runner.
#[tauri::command]
pub async fn lab_check(app: AppHandle, id: String, runtime: Runtime) -> Result<docker::Check> {
    let dir = lab_dir(&app, &id)?;
    let home = || runner_home(&app);
    match server::lab_connection(&app, &dir)? {
        // On a server host or cloud account: checks run through the runner (Proxmox over SSH via
        // the lab's router; ESXi from its controller; a cloud over SSH to the lab).
        Some(c) if c.provider == super::providers::Provider::VmwareEsxi => super::launch::esxi_run(&app, &dir, &id, runtime, &c)?.check().await,
        Some(c) if c.provider.is_cloud() => {
            let cloud = server::terraform_target(c.provider).unwrap_or_default();
            let place = isothrum::CloudPlace { region: c.region.clone(), tf_env: c.tf_env.clone(), ..Default::default() };
            isothrum::CloudRun::new(&home()?, &dir, &id, runtime, cloud)?.check(&place).await
        }
        Some(_) => isothrum::ServerRun::new(&home()?, &dir, &id, runtime)?.check().await,
        // On this machine, through the runner.
        None if runtime == Runtime::Docker && local_vm(&dir).is_some() => isothrum::LabRun::docker_in_vm(&home()?, &dir, &id)?.check().await,
        None if runtime == Runtime::Docker => isothrum::LabRun::docker(&home()?, &dir, &id)?.check().await,
        None if runtime == Runtime::Vm => isothrum::LabRun::vm(&home()?, &dir, &id)?.check().await,
        None => Ok(docker::Check { available: false, ok: false, output: String::new(), results: Vec::new() }),
    }
}

/// The local run of lab `id`, or `None` when the lab runs on a server host or cloud account (no
/// attack box on this machine then — the learner works from what the lab gives them). Mirrors the
/// local dispatch of [`lab_check`].
fn local_run(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<isothrum::LabRun>> {
    if server::lab_connection(app, dir)?.is_some() {
        return Ok(None);
    }
    let home = runner_home(app)?;
    Ok(match runtime {
        Runtime::Docker if local_vm(dir).is_some() => Some(isothrum::LabRun::docker_in_vm(&home, dir, id)?),
        Runtime::Docker => Some(isothrum::LabRun::docker(&home, dir, id)?),
        Runtime::Vm => Some(isothrum::LabRun::vm(&home, dir, id)?),
        Runtime::Cloud => None,
    })
}

/// The lab's run with an attack box on this machine, or an error when the lab has none here.
fn with_attack_box(app: &AppHandle, dir: &Path, id: &str, runtime: Runtime) -> Result<isothrum::LabRun> {
    local_run(app, dir, id, runtime)?.ok_or_else(|| {
        Error::Invalid(
            "The attack box for a lab on a server or in a cloud account is reached through the lab itself, not from here. Work from what the lab gives you."
                .into(),
        )
    })
}

/// The command the runner builds to open a shell into lab `id`'s attack box (the in-app terminal
/// and the system terminal both run it).
pub fn box_shell_command(app: &AppHandle, id: &str, runtime: Runtime) -> Result<isothrum_runner::ShellCommand> {
    let dir = lab_dir(app, id)?;
    with_attack_box(app, &dir, id, runtime)?.shell_command()
}

/// The attack box's status for the lab detail view: the lab's on-demand access machine as the
/// runner reports it, and the lab networks it can join (for the learner to pick from). Boxed:
/// the runner's status future is large, and awaiting it inline would make every caller's future
/// large too.
#[tauri::command]
pub async fn attack_box_status(app: AppHandle, id: String, runtime: Runtime, network: Option<String>) -> Result<isothrum::AttackStatus> {
    Box::pin(async move {
        let dir = lab_dir(&app, &id)?;
        match local_run(&app, &dir, &id, runtime)? {
            Some(run) => run.attack_status(network.as_deref()).await,
            None => Ok(isothrum::AttackStatus::default()),
        }
    })
    .await
}

/// The learner's pick for an attach: which lab network to join (none: the only one), with which
/// box image. One argument so the command stays within Tauri's argument count.
#[derive(serde::Deserialize)]
pub struct AttachPick {
    network: Option<String>,
    image: String,
}

/// Hot-adds the attack box into the running lab, on the only network or the one the learner
/// picked, with the box image they chose; the runner pulls the image on the first attach.
#[tauri::command]
pub async fn attack_box_attach(app: AppHandle, id: String, runtime: Runtime, pick: AttachPick, logs: Channel<String>) -> Result<()> {
    Box::pin(async move {
        let AttachPick { network, image } = pick;
        let dir = lab_dir(&app, &id)?;
        check_image(&image)?;
        let run = with_attack_box(&app, &dir, &id, runtime)?;
        // Boxed: the runner's attach future is large, and tracking it adds to it.
        Box::pin(crate::machine::storage::owned::tracked(
            &app,
            std::slice::from_ref(&image),
            &[],
            run.attach(network.as_deref(), Some(&image), &mut channel_log(logs)),
        ))
        .await
    })
    .await
}

/// Takes the attack box back out of the running lab, leaving the rest of the lab up.
#[tauri::command]
pub async fn attack_box_detach(app: AppHandle, id: String, runtime: Runtime, logs: Channel<String>) -> Result<()> {
    Box::pin(async move {
        let dir = lab_dir(&app, &id)?;
        with_attack_box(&app, &dir, &id, runtime)?.detach(&mut channel_log(logs)).await
    })
    .await
}

/// Opens the attack box shell of a running lab in the system terminal, on the command the runner
/// builds (it alone knows the Compose project and files, the Vagrant define).
#[tauri::command]
pub async fn lab_attack_shell(app: AppHandle, id: String, runtime: Runtime) -> Result<()> {
    let dir = lab_dir(&app, &id)?;
    let shell = with_attack_box(&app, &dir, &id, runtime)?.shell_command()?;
    terminal_launch::open_terminal_async(shell).await
}

#[cfg(test)]
mod tests {
    use super::{AttachPick, channel_log, check_image, valid_machine_name};
    use super::{
        attack_box_attach, attack_box_detach, attack_box_status, lab_attack_shell, lab_check, lab_park, lab_provision, lab_resume, lab_status, lab_stop,
        lab_tools, running_labs,
    };
    use crate::platform::exec::fake::Tools;
    use crate::runtime::{Park, Runtime, server};
    use crate::testing::test_app::TestApp;
    use tauri::ipc::{Channel, InvokeResponseBody};

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";

    /// Starts an installed lab on Docker the way a launch does, minus the worker and the token.
    async fn start_docker(t: &TestApp, id: &str, logs: Channel<String>) -> crate::error::Result<()> {
        let dir = super::lab_dir(t.handle(), id)?;
        crate::runtime::start(t.handle(), &dir, id, Runtime::Docker, None, None, &[], super::channel_log(logs)).await
    }

    fn sink() -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
    }

    #[tokio::test]
    async fn running_labs_lists_nothing_when_no_lab_is_running() {
        // An installed but never-started lab is not running; the quit guard and the tray read
        // this. A running lab's listing is covered by the real-lab integration tests.
        let t = TestApp::new();
        t.lab("web", &[("isoloom.yml", SPEC)]);
        let _tools = Tools::rules(&[]);
        assert!(running_labs(t.handle().clone()).await.is_empty());
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
        assert_eq!(lab_status(app(), "gone".into(), Runtime::Vm).await.err().unwrap().to_string(), missing);
        assert!(lab_tools(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_check(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(lab_attack_shell(app(), "gone".into(), Runtime::Docker).await.is_err());
        assert!(attack_box_status(app(), "gone".into(), Runtime::Docker, None).await.is_err());
        assert!(
            attack_box_attach(app(), "gone".into(), Runtime::Docker, AttachPick { network: None, image: "kalilinux/kali-rolling".into() }, channel(s.clone()))
                .await
                .is_err()
        );
        assert!(attack_box_detach(app(), "gone".into(), Runtime::Docker, channel(s.clone())).await.is_err());
        // Installed, but a machine name or image that could reach a command line is refused.
        t.lab("ad", &[]);
        let err = lab_provision(app(), "ad".into(), Runtime::Vm, Some("a;rm".into()), channel(s.clone())).await.unwrap_err().to_string();
        assert_eq!(err, "invalid machine name `a;rm`");
        assert_eq!(
            attack_box_attach(app(), "ad".into(), Runtime::Vm, AttachPick { network: None, image: "-x/y z".into() }, channel(s.clone()))
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
        let st = attack_box_status(t.handle().clone(), "web".into(), Runtime::Docker, None).await.unwrap();
        assert!(!st.running && st.ip.is_empty() && st.networks.is_empty());
        // A lab on a server host has no attack box here: status is empty, attach/detach refuse.
        let dir = t.lab("srv", &[]);
        server::mark_lab(&dir, Some(&t.host("az1", "azure", serde_json::json!({})))).unwrap();
        let s = sink();
        assert!(!attack_box_status(t.handle().clone(), "srv".into(), Runtime::Docker, None).await.unwrap().running);
        assert!(
            attack_box_attach(
                t.handle().clone(),
                "srv".into(),
                Runtime::Docker,
                AttachPick { network: None, image: "kalilinux/kali-rolling".into() },
                channel(s.clone())
            )
            .await
            .is_err()
        );
        assert!(attack_box_detach(t.handle().clone(), "srv".into(), Runtime::Docker, channel(s.clone())).await.is_err());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(s.lock().unwrap().is_empty());
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
        let log = channel_log(channel(sink.clone()));
        log("Starting…".into());
        log("Done".into());
        assert_eq!(*sink.lock().unwrap(), ["\"Starting…\"", "\"Done\""]);
    }

    #[tokio::test]
    async fn attack_box_commands_refuse_bad_ids_and_images_before_touching_the_runner() {
        let t = TestApp::new();
        assert!(attack_box_status(t.handle().clone(), "../x".into(), Runtime::Docker, None).await.is_err());
        let sink = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        t.lab("web", &[("isoloom.yml", SPEC)]);
        assert!(
            attack_box_attach(
                t.handle().clone(),
                "a b".into(),
                Runtime::Docker,
                AttachPick { network: None, image: "kalilinux/kali-rolling".into() },
                channel(sink.clone())
            )
            .await
            .is_err()
        );
        let bad_image =
            attack_box_attach(t.handle().clone(), "web".into(), Runtime::Docker, AttachPick { network: None, image: "x;y".into() }, channel(sink.clone()))
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
}
