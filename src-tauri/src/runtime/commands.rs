//! The Tauri commands over lab runtimes. They validate what the UI passes (ids, images, boxes,
//! machine names), hand long operations to the detached deploy worker, and stream logs back
//! over a channel.

use crate::AppHandle;
use tauri::ipc::Channel;

use super::lifecycle::stop_lab;
use super::paths::{lab_dir, local_vm, runner_home, validate_id};
use super::shell::lab_shell_command;
use super::{LabStatus, Park, Runtime, attack_vm, cloud_services, discover, docker, exegol, isothrum, lab, server, status};
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

fn check_box(box_name: &str) -> Result<()> {
    if attack_vm::valid_box(box_name) { Ok(()) } else { Err(Error::Invalid(format!("invalid attack VM box `{box_name}` (expected owner/name)"))) }
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
    let spec = lab::instanced(&dir)?;
    Ok(lab::tools(&spec, runtime == Runtime::Docker))
}

/// The ids of labs running on this machine right now, found by scanning Docker and Vagrant
/// directly. Unlike `lab_status` (one lab at a time, from its dir and Compose file), this is a
/// single infrastructure scan, so the UI recovers which labs are up even after a crash or restart,
/// and a lab whose per-lab status probe is momentarily failing still shows as running.
#[tauri::command]
pub async fn running_labs(app: AppHandle) -> Vec<String> {
    discover::running_lab_ids(&app).await
}

/// Runs a lab's exploitability check: does the intended exploit path still work? Lets a
/// learner who broke their box know to reset it. Local Docker labs only for now.
#[tauri::command]
pub async fn lab_check(app: AppHandle, id: String, runtime: Runtime) -> Result<docker::Check> {
    let dir = lab_dir(&app, &id)?;
    match runtime {
        Runtime::Docker if server::lab_connection(&app, &dir)?.is_none() && local_vm(&dir).is_none() && isothrum::chosen() => {
            isothrum::DockerRun::new(&runner_home(&app)?, &dir, &id)?.check().await
        }
        Runtime::Docker if server::lab_connection(&app, &dir)?.is_none() => docker::check(&dir, &id).await,
        _ => Ok(docker::Check { available: false, ok: false, output: String::new(), results: Vec::new() }),
    }
}

/// Opens the attack box shell of a running lab in the system terminal.
#[tauri::command]
pub async fn lab_attack_shell(app: AppHandle, id: String, runtime: Runtime) -> Result<()> {
    exegol::open_terminal_async(lab_shell_command(&app, &id, runtime, None).await?.0).await
}

/// Status of a lab's attack box (Exegol), for the lab detail view.
#[tauri::command]
pub async fn exegol_status(id: String, image: String) -> Result<exegol::ExegolStatus> {
    validate_id(&id)?;
    check_image(&image)?;
    Ok(exegol::status(&id, &image).await)
}

/// Launches the attack box on the lab's network (pulls the image first if needed).
#[tauri::command]
pub async fn exegol_start(app: AppHandle, id: String, image: String, logs: Channel<String>) -> Result<()> {
    validate_id(&id)?;
    check_image(&image)?;
    crate::machine::storage::owned::tracked(&app, std::slice::from_ref(&image), &[], exegol::start(&id, &image, channel_log(logs))).await
}

#[tauri::command]
pub async fn exegol_stop(id: String, logs: Channel<String>) -> Result<()> {
    validate_id(&id)?;
    exegol::stop(&id, channel_log(logs)).await
}

/// The attack VM beside a VM lab on this machine, in the container attack box's status shape.
#[tauri::command]
pub async fn attack_vm_status(app: AppHandle, id: String, box_name: String) -> Result<exegol::ExegolStatus> {
    let dir = lab_dir(&app, &id)?;
    check_box(&box_name)?;
    attack_vm::status(&dir, &box_name).await
}

/// Starts the attack VM beside a running VM lab, on the lab's hypervisor (the first start
/// downloads the box). Runs detached (a worker), so a quit or a rebuild never cuts it short.
#[tauri::command]
pub async fn attack_vm_start(app: AppHandle, id: String, box_name: String, logs: Channel<String>) -> Result<()> {
    lab_dir(&app, &id)?;
    check_box(&box_name)?;
    deploy_worker::run_job(&app, Job::on_lab(&id, Op::AttackVm { box_name }), logs).await
}

#[tauri::command]
pub async fn attack_vm_stop(app: AppHandle, id: String, logs: Channel<String>) -> Result<()> {
    let dir = lab_dir(&app, &id)?;
    // A start still downloading or booting it is stopped first, so the removal is final.
    deploy_worker::kill_and_wait(&app, &deploy_worker::attack_key(&id)).await;
    attack_vm::stop(&dir, channel_log(logs)).await
}

/// Opens the player's terminal on an SSH session into the attack VM.
#[tauri::command]
pub async fn attack_vm_shell(app: AppHandle, id: String) -> Result<()> {
    let dir = lab_dir(&app, &id)?;
    // Off the UI thread: finding a terminal that starts can take seconds.
    tokio::task::spawn_blocking(move || attack_vm::shell(&dir)).await.map_err(|e| Error::Invalid(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::{
        attack_vm_shell, attack_vm_start, attack_vm_status, attack_vm_stop, lab_attack_shell, lab_check, lab_park, lab_provision, lab_resume, lab_status,
        lab_stop, lab_tools, running_labs,
    };
    use super::{channel_log, check_box, check_image, exegol_start, exegol_status, exegol_stop, valid_machine_name};
    use crate::platform::exec::fake::{Tools, ok};
    use crate::runtime::{Park, Runtime, server};
    use crate::testing::test_app::TestApp;
    use tauri::ipc::{Channel, InvokeResponseBody};

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\n";

    /// Starts an installed lab on Docker the way a launch does, minus the worker and the token.
    async fn start_docker(t: &TestApp, id: &str, logs: Channel<String>) -> crate::error::Result<()> {
        let dir = super::lab_dir(t.handle(), id)?;
        crate::runtime::start(t.handle(), &dir, id, Runtime::Docker, None, None, &[], super::channel_log(logs)).await
    }

    fn sink() -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
    }

    #[tokio::test]
    async fn lab_commands_start_report_and_stop_an_installed_lab() {
        let t = TestApp::new();
        t.lab("web", &[("isoloom.yml", SPEC)]);
        let app = || t.handle().clone();
        let tools = Tools::rules(&[("ps --all", ok(r#"{"Service":"web","State":"exited"}"#))]);
        let logs = sink();
        start_docker(&t, "web", channel(logs.clone())).await.unwrap();
        assert!(tools.called("up -d"));
        let s = lab_status(app(), "web".into(), Runtime::Docker).await.unwrap();
        assert!(!s.running);
        assert_eq!(lab_tools(app(), "web".into(), Runtime::Docker).await.unwrap().len(), 0);
        let check = lab_check(app(), "web".into(), Runtime::Docker).await.unwrap();
        assert!(!check.ok);
        let _ = running_labs(app()).await;
        lab_stop(app(), "web".into(), Runtime::Docker, channel(logs.clone())).await.unwrap();
        assert!(tools.called("down --volumes"));
        assert!(!logs.lock().unwrap().is_empty());
        // A VM lab's status is its VMs'.
        t.lab("ad", &[]);
        assert!(!lab_status(app(), "ad".into(), Runtime::Vm).await.unwrap().running);
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
        assert!(attack_vm_status(app(), "gone".into(), "kalilinux/rolling".into()).await.is_err());
        assert!(attack_vm_start(app(), "gone".into(), "kalilinux/rolling".into(), channel(s.clone())).await.is_err());
        assert!(attack_vm_stop(app(), "gone".into(), channel(s.clone())).await.is_err());
        assert!(attack_vm_shell(app(), "gone".into()).await.is_err());
        // Installed, but a machine name or box that could reach a command line is refused.
        t.lab("ad", &[]);
        let err = lab_provision(app(), "ad".into(), Runtime::Vm, Some("a;rm".into()), channel(s.clone())).await.unwrap_err().to_string();
        assert_eq!(err, "invalid machine name `a;rm`");
        assert!(attack_vm_status(app(), "ad".into(), "rolling".into()).await.is_err());
        assert!(attack_vm_start(app(), "ad".into(), "-x/y z".into(), channel(s.clone())).await.is_err());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(s.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_attack_vm_commands_read_and_remove_it() {
        let t = TestApp::new();
        t.lab("ad", &[]);
        let tools = Tools::rules(&[("box list", ok("kalilinux/rolling (virtualbox, 2024.4.0)\n"))]);
        let s = attack_vm_status(t.handle().clone(), "ad".into(), "kalilinux/rolling".into()).await.unwrap();
        assert!(s.image_present && !s.running);
        attack_vm_stop(t.handle().clone(), "ad".into(), channel(sink())).await.unwrap();
        // Never started: no shell to open.
        let err = attack_vm_shell(t.handle().clone(), "ad".into()).await.unwrap_err().to_string();
        assert_eq!(err, "the attack VM isn't started");
        assert!(tools.called("vagrant box list"));
    }

    #[tokio::test]
    async fn attack_box_commands_drive_docker_for_a_valid_lab() {
        let t = TestApp::new();
        let tools = Tools::rules(&[("inspect", ok("true\t10.0.0.5\n"))]);
        let s = sink();
        let st = exegol_status("web-1".into(), "nwodtuhs/exegol:free".into()).await.unwrap();
        assert!(st.running);
        exegol_stop("web-1".into(), channel(s.clone())).await.unwrap();
        assert!(tools.called("docker rm -f cyberctf-web-1-attacker"));
        let _ = exegol_start(t.handle().clone(), "web-1".into(), "nwodtuhs/exegol:free".into(), channel(s.clone())).await;
        assert!(tools.called("nwodtuhs/exegol:free"));
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
    async fn attack_box_commands_refuse_bad_ids_and_images_before_touching_docker() {
        let t = TestApp::new();
        let bad_id = exegol_status("../x".into(), "nwodtuhs/exegol:free".into()).await.err().unwrap().to_string();
        assert_eq!(bad_id, "invalid lab id `../x`");
        let bad_image = exegol_status("web-1".into(), "-x".into()).await.err().unwrap().to_string();
        assert_eq!(bad_image, "invalid attack-box image `-x`");
        let sink = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        assert!(exegol_start(t.handle().clone(), "a b".into(), "nwodtuhs/exegol:free".into(), channel(sink.clone())).await.is_err());
        assert!(exegol_start(t.handle().clone(), "web-1".into(), "x;y".into(), channel(sink.clone())).await.is_err());
        assert!(exegol_stop("a;rm".into(), channel(sink.clone())).await.is_err());
        assert!(sink.lock().unwrap().is_empty(), "nothing ran");
    }

    #[tokio::test]
    async fn a_stop_reads_as_stopping_from_the_moment_it_is_asked_for() {
        // Regression guard: ending a deploy can take minutes (a Terraform apply is interrupted
        // and waited for, then the lab lock); the page and the quit guard must see a stop, not
        // a bare lab, the whole time.
        let t = TestApp::new();
        let id = "commands-test-stop-waits";
        t.lab(id, &[]);
        let tools = Tools::rules(&[]);
        let held = crate::runtime::inflight::lock_lab(t.handle(), id).await.unwrap();
        let app = t.handle().clone();
        let logs = sink();
        let ch = channel(logs.clone());
        let mut stop = tokio::spawn(async move { lab_stop(app, id.into(), Runtime::Docker, ch).await });
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), &mut stop).await.is_err(), "the stop waits for the lab");
        let action = |id: &str| crate::runtime::active_actions().into_iter().find(|(l, _)| l == id).map(|(_, a)| a);
        assert_eq!(action(id), Some(crate::runtime::Action::Stop));
        assert!(!tools.called("down --volumes"));
        drop(held);
        stop.await.unwrap().unwrap();
        assert!(tools.called("down --volumes"));
        assert_eq!(action(id), None);
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
    fn image_and_box_errors_name_what_was_wrong() {
        assert!(check_image("nwodtuhs/exegol:free").is_ok());
        assert_eq!(check_image("-x").unwrap_err().to_string(), "invalid attack-box image `-x`");
        assert!(check_box("kalilinux/rolling").is_ok());
        assert_eq!(check_box("rolling").unwrap_err().to_string(), "invalid attack VM box `rolling` (expected owner/name)");
    }
}
