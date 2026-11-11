//! What a player does that breaks a running or starting lab, on the real Docker: a container
//! removed (`docker rm -f`) or stopped (`docker stop`) while the lab runs, one removed while the
//! start waits for it, the process running the start killed (SIGKILL), a port the lab publishes
//! taken by another process, and (on a throwaway machine only) Docker itself stopped mid-start.
//! Each scenario does the disruption with the tool a player would use, then checks what the
//! launcher says (the lab page's status, the failure's words), that a start again (or the lab
//! page's resume, or its reset: stop then start) brings the whole lab back, and that its stop
//! leaves nothing behind. tests/resilience.md is the catalogue these come from; the VM family is
//! `resilience_vm`.
//!
//! Every scenario acts only on what it made: its own lab copy (`cyberctf-resil-<n>`), its
//! containers, found by their Compose project, and a port it binds itself. The daemon one stops
//! Docker for everything on the machine: it refuses to run unless `CYBERCTF_TEST_RESILIENCE_THROWAWAY`
//! is `1`, which only the test rig sets on its throwaway VMs.
//!
//! ```text
//! CYBERCTF_TEST_RESILIENCE_LAB=<isoloom checkout>/examples/hello-stack \
//! cargo test --lib integration_tests::resilience:: -- --ignored --nocapture --test-threads=1
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::support::lab::{Run, install_copy, leftovers};
use super::support::{check, fetch, knob, log, scenario, var};
use crate::platform::exec::run;
use crate::runtime::{LabStatus, docker, isothrum};

/// The machine a scenario disrupts: `CYBERCTF_TEST_RESILIENCE_MACHINE`, else `web`.
pub(super) fn target() -> String {
    var("CYBERCTF_TEST_RESILIENCE_MACHINE").unwrap_or_else(|| "web".into())
}

/// Whether this is a throwaway test machine, where a scenario may stop Docker or a hypervisor's
/// VM by its own tools: said, and false, when `CYBERCTF_TEST_RESILIENCE_THROWAWAY` isn't `1`.
pub(super) fn throwaway() -> bool {
    let set = var("CYBERCTF_TEST_RESILIENCE_THROWAWAY").is_some_and(|v| v == "1");
    if !set {
        // The runner records this as skipped: the scenario never touches a machine someone uses.
        println!(
            "{} CYBERCTF_TEST_RESILIENCE_THROWAWAY (this scenario stops more than its own lab: the test rig's throwaway machines only)",
            super::support::SKIPPED
        );
    }
    set
}

/// `command` run by the system's shell (`sh -c`, or PowerShell on Windows): a knob's command,
/// as the rig writes it for its OS.
pub(super) async fn shell(command: &str) -> crate::error::Result<String> {
    if cfg!(windows) { run("powershell", &["-NoProfile", "-Command", command], None).await } else { run("sh", &["-c", command], None).await }
}

/// The container of service `service` of lab `id`'s Compose project, if it has one.
async fn container_of(id: &str, service: &str) -> Option<String> {
    let project = format!("label=com.docker.compose.project={}", docker::project(id));
    let service = format!("label=com.docker.compose.service={service}");
    let out = run("docker", &["ps", "-aq", "--filter", &project, "--filter", &service], None).await.ok()?;
    out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_owned)
}

/// The lab's own machines in `s`, each with its state.
pub(super) fn states(s: &LabStatus) -> Vec<(String, String)> {
    s.machines.iter().filter(|m| !m.infra).map(|m| (m.name.clone(), m.state.clone())).collect()
}

/// Whether the lab reads as up, every one of its machines running.
pub(super) fn whole(s: &LabStatus) -> bool {
    s.running && !s.machines.is_empty() && s.machines.iter().filter(|m| !m.infra).all(|m| m.state == "running")
}

/// The lab started (as the lab page's Start does), then up, its URL answering: each a check.
async fn starts_whole(l: &Run, f: &mut Vec<String>, when: &str) -> bool {
    let started = l.start(&[], &mut log(&l.id)).await;
    whole_after(started, async || l.status().await, f, when).await
}

/// Whether a start that ended `started` went through, then (when it did) its lab as `status`
/// reads it up and answering: each a check.
pub(super) async fn whole_after(
    started: crate::error::Result<()>,
    status: impl AsyncFnOnce() -> crate::error::Result<LabStatus>,
    f: &mut Vec<String>,
    when: &str,
) -> bool {
    check(started.is_ok(), format!("{when}: it starts ({:?})", started.as_ref().err()), f);
    if started.is_err() {
        return false;
    }
    up_and_answering(status().await, f, when).await;
    true
}

/// `s`, a lab's status just after its start: every machine running, its URL answering.
async fn up_and_answering(s: crate::error::Result<LabStatus>, f: &mut Vec<String>, when: &str) {
    check(s.as_ref().is_ok_and(whole), format!("{when}: every machine runs ({:?})", s.as_ref().map(states)), f);
    if let Some(url) = s.ok().and_then(|s| s.url) {
        let answered = fetch(&url).await;
        check(answered.as_ref().is_ok_and(reqwest::StatusCode::is_success), format!("{when}: {url} answers ({answered:?})"), f);
    }
}

/// The lab started whole, then its machine under test ([`target`]) done `to` with Docker
/// (`rm -f`, `stop`): the machine's name, once done.
async fn started_then(l: &Run, to: &[&str], f: &mut Vec<String>) -> Option<String> {
    if !starts_whole(l, f, "first").await {
        return None;
    }
    let machine = target();
    let Some(container) = container_of(&l.id, &machine).await else {
        check(false, format!("{machine} has a container"), f);
        return None;
    };
    let done = run("docker", &[to, &[container.as_str()]].concat(), None).await;
    check(done.is_ok(), format!("docker {} {machine} ({:?})", to.join(" "), done.err()), f);
    Some(machine)
}

/// The lab stopped (the lab page's Stop), and nothing of it left on Docker.
async fn stops_clean(l: &Run, f: &mut Vec<String>) {
    let stopped = l.stop(log(&l.id)).await;
    check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
    let left = leftovers(&l.id).await;
    check(left.is_empty(), format!("nothing of it is left ({left:?})"), f);
    let _ = std::fs::remove_dir_all(&l.dir);
    let _ = std::fs::remove_dir_all(&l.home);
}

/// Runs `body` on a fresh copy of the lab in `CYBERCTF_TEST_RESILIENCE_LAB`, then stops it and
/// checks nothing is left, whatever happened; fails the test on any failed check.
async fn on_lab(name: &str, body: impl AsyncFnOnce(&Run, &mut Vec<String>)) {
    let Some(src) = knob("CYBERCTF_TEST_RESILIENCE_LAB") else { return };
    let l = copy_of(&src, "resil");
    let mut f = Vec::new();
    scenario(&mut f, async |f: &mut Vec<String>| body(&l, f).await, async |f: &mut Vec<String>| stops_clean(&l, f).await).await;
    assert!(f.is_empty(), "{name}:\n{}", f.join("\n"));
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_container_removed_while_the_lab_runs_shows_down_and_a_reset_brings_it_back() {
    on_lab("removed while running", async |l: &Run, f: &mut Vec<String>| {
        let Some(machine) = started_then(l, &["rm", "-f"], f).await else { return };
        // The lab page: still running, the machine shown down (gone), not left out.
        let s = l.status().await;
        let shown = s.as_ref().ok().and_then(|s| s.machines.iter().find(|m| m.name == machine).map(|m| m.state.clone()));
        check(s.as_ref().is_ok_and(|s| s.running), format!("the lab still reads as running ({:?})", s.as_ref().map(states)), f);
        check(shown.as_deref() == Some("absent"), format!("{machine} is shown gone ({shown:?})"), f);
        // A resume can't bring back what is gone: refused, naming it.
        let resumed = l.resume(&mut log(&l.id)).await;
        let said = resumed.as_ref().err().map(ToString::to_string).unwrap_or_default();
        check(said.contains("machines are gone") && said.contains(&machine), format!("a resume is refused, naming {machine} ({said:?})"), f);
        // The way back: the lab page's reset, a stop then a start.
        let stopped = l.stop(log(&l.id)).await;
        check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
        starts_whole(l, f, "after the reset").await;
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_container_stopped_while_the_lab_runs_shows_down_and_a_resume_brings_it_back() {
    on_lab("stopped while running", async |l: &Run, f: &mut Vec<String>| {
        let Some(machine) = started_then(l, &["stop"], f).await else { return };
        let s = l.status().await;
        let shown = s.as_ref().ok().and_then(|s| s.machines.iter().find(|m| m.name == machine).map(|m| m.state.clone()));
        check(shown.as_deref().is_some_and(|st| st != "running"), format!("{machine} is shown down ({shown:?})"), f);
        let resumed = l.resume(&mut log(&l.id)).await;
        check(resumed.is_ok(), format!("a resume brings it back ({:?})", resumed.err()), f);
        let s = l.status().await;
        check(s.as_ref().is_ok_and(whole), format!("every machine runs again ({:?})", s.as_ref().map(states)), f);
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_container_removed_while_the_start_waits_fails_it_or_shows_it_and_a_start_again_works() {
    on_lab("removed while starting", async |l: &Run, f: &mut Vec<String>| {
        let machine = target();
        // The removal races the start: as soon as the machine's container exists, it goes.
        let remove = async {
            let deadline = Instant::now() + Duration::from_secs(600);
            while Instant::now() < deadline {
                if let Some(c) = container_of(&l.id, &machine).await {
                    return run("docker", &["rm", "-f", &c], None).await.is_ok();
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            false
        };
        let mut said = log(&l.id);
        let (started, removed) = tokio::join!(l.start(&[], &mut said), remove);
        check(removed, format!("{machine}'s container was removed while the start ran"), f);
        let s = l.status().await;
        if let Err(e) = &started {
            // The start saw it go: it failed, saying so, and the lab isn't read as whole.
            println!("note: the start failed: {e}");
            check(e.to_string().contains("No such container"), "the failure says the container went (Docker's words)".into(), f);
            check(!s.as_ref().is_ok_and(whole), format!("the lab isn't read as whole ({:?})", s.as_ref().map(states)), f);
        } else {
            // It went after the start's wait: the page shows it gone.
            let shown = s.as_ref().ok().and_then(|s| s.machines.iter().find(|m| m.name == machine).map(|m| m.state.clone()));
            check(shown.as_deref() == Some("absent"), format!("{machine} is shown gone ({shown:?})"), f);
            let stopped = l.stop(log(&l.id)).await;
            check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
        }
        // A start again clears what the first left, and brings the whole lab up.
        starts_whole(l, f, "started again").await;
    })
    .await;
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_port_the_lab_publishes_taken_at_start_is_named_and_a_start_once_free_works() {
    on_lab("port taken", async |l: &Run, f: &mut Vec<String>| {
        // The port the lab publishes with its own ports (its instance's: 8180 for hello-stack's
        // 8080 on a second instance), read from a first start.
        let fixed = crate::runtime::launch::Inputs { values: &[], default_ports: true };
        let first = crate::runtime::launch::start_docker_run(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
        check(first.is_ok(), format!("it starts on its own ports ({:?})", first.as_ref().err()), f);
        let url = l.status().await.ok().and_then(|s| s.url);
        let Some(port) = url.as_deref().and_then(|u| u.rsplit(':').next()).and_then(|p| p.trim_end_matches('/').parse::<u16>().ok()) else {
            return check(false, format!("it publishes a port ({url:?})"), f);
        };
        let stopped = l.stop(log(&l.id)).await;
        check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
        // Another process holds it now.
        let Ok(held) = std::net::TcpListener::bind(("127.0.0.1", port)) else {
            return check(false, format!("port {port} could be held by the test"), f);
        };
        let refused = crate::runtime::launch::start_docker_run(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
        let code = refused.as_ref().err().map(crate::error::Error::code);
        check(
            matches!(code, Some(crate::error::Code::PortInUse { port: p, .. }) if p == port),
            format!("the start is refused for port {port}, named ({:?})", refused.as_ref().err().map(ToString::to_string)),
            f,
        );
        let left = leftovers(&l.id).await;
        check(left.is_empty(), format!("the refused start left nothing ({left:?})"), f);
        drop(held);
        let started = crate::runtime::launch::start_docker_run(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
        check(started.is_ok(), format!("once the port is free it starts ({:?})", started.err()), f);
    })
    .await;
}

/// What the killed start's child is handed: `CYBERCTF_TEST_RESILIENCE_CHILD`, one line each:
/// the form (`docker`, `vm`), the runner home, the lab folder, its id, the hypervisor.
const CHILD: &str = "CYBERCTF_TEST_RESILIENCE_CHILD";

/// The test the child runs ([`the_child_starts_the_lab_it_is_handed`]).
const CHILD_TEST: &str = "integration_tests::resilience::the_child_starts_the_lab_it_is_handed";

/// Not a scenario of its own: the process a killed-start scenario starts and kills
/// ([`killed_start`]). It starts the lab it is handed in `CYBERCTF_TEST_RESILIENCE_CHILD`, as the
/// deploy worker does, and returns early without it.
#[tokio::test]
#[ignore = "run by the killed-start scenarios, as their child process"]
async fn the_child_starts_the_lab_it_is_handed() {
    let Some(handed) = var(CHILD) else { return };
    let [form, home, dir, id, provider] = handed.lines().collect::<Vec<_>>()[..] else { panic!("{CHILD}: {handed}") };
    let (home, dir) = (Path::new(home), Path::new(dir));
    let started = match form {
        "vm" => {
            let provider = serde_json::from_value(serde_json::json!(provider)).expect("a hypervisor");
            let inputs = super::support::lab::vm_inputs();
            let start = crate::runtime::launch::VmStart { provider, inputs: crate::runtime::launch::Inputs::values(&inputs) };
            crate::runtime::launch::start_vm_run(home, dir, id, start, &mut log(id)).await
        }
        _ => crate::runtime::launch::start_docker_run(home, dir, id, crate::runtime::launch::Inputs::default(), &mut log(id)).await,
    };
    println!("child: the start ended before it was killed: {started:?}");
}

/// The processes still running a tool for run `name` (its name on their command line or in their
/// environment: Compose's `--project-name`, Vagrant's `VAGRANT_DOTFILE_PATH`), but this one.
pub(super) fn tools_of(name: &str) -> Vec<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_environ(UpdateKind::Always));
    let me = std::process::id();
    let has = |s: &std::ffi::OsString| s.to_string_lossy().contains(name);
    sys.processes()
        .values()
        .filter(|p| p.pid().as_u32() != me && (p.cmd().iter().any(has) || p.environ().iter().any(has)))
        .map(|p| format!("{} {}", p.pid(), p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")))
        .collect()
}

/// A start of lab `l` in `form` on `provider` run in a child process, killed (SIGKILL; on
/// Windows, terminated) once `begun` says it has made something; then what it leaves: the run
/// read as failed (never ready), the tools it started let finish (said: a killed process can't
/// stop them), and a start again in this process that brings the whole lab up.
pub(super) async fn killed_start(l: &Run, form: &str, provider: &str, begun: impl AsyncFn() -> bool, f: &mut Vec<String>) {
    // Every line has a value: an empty last one would be dropped.
    let provider = if provider.is_empty() { "-" } else { provider };
    let handed = [form, &l.home.to_string_lossy(), &l.dir.to_string_lossy(), &l.id, provider].join("\n");
    let exe = std::env::current_exe().expect("this test binary");
    let mut child =
        match std::process::Command::new(exe).args(["--ignored", "--exact", "--nocapture", "--test-threads=1", CHILD_TEST]).env(CHILD, handed).spawn() {
            Ok(c) => c,
            Err(e) => return check(false, format!("the child start runs ({e})"), f),
        };
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut made = false;
    while Instant::now() < deadline && child.try_wait().ok().flatten().is_none() {
        if begun().await {
            made = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    check(made, "the child's start made something before it was killed".into(), f);
    let killed = child.kill();
    let _ = child.wait();
    check(killed.is_ok(), format!("the child is killed mid-start ({:?})", killed.err()), f);
    // The tools it started are in process groups of their own: they outlive it, and finish.
    let name = docker::project(&l.id);
    let orphans = tools_of(&name);
    println!("note: tools the killed start left running: {orphans:?}");
    let wait = Instant::now() + Duration::from_secs(if form == "vm" { 3600 } else { 600 });
    while Instant::now() < wait && !tools_of(&name).is_empty() {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    check(tools_of(&name).is_empty(), format!("the killed start's tools ended ({:?})", tools_of(&name)), f);
    // The runner: a run whose start never ended is failed, not ready.
    let run = if form == "vm" { isothrum::LabRun::vm(&l.home, &l.dir, &l.id) } else { isothrum::LabRun::docker(&l.home, &l.dir, &l.id) };
    let state = match run {
        Ok(run) => run.run_status().await.map(|s| s.map(|s| s.state)),
        Err(e) => Err(e),
    };
    check(
        matches!(state, Ok(Some(isothrum_runner::RunState::Failed | isothrum_runner::RunState::Absent))),
        format!("the killed start's run reads as failed, not ready ({state:?})"),
        f,
    );
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_start_killed_midway_is_failed_and_a_start_again_works() {
    on_lab("killed start", async |l: &Run, f: &mut Vec<String>| {
        let begun = async || leftovers(&l.id).await.iter().any(|c| c.starts_with("container "));
        killed_start(l, "docker", "", begun, f).await;
        starts_whole(l, f, "started again after the kill").await;
    })
    .await;
}

#[tokio::test]
#[ignore = "stops Docker: a throwaway machine (CYBERCTF_TEST_RESILIENCE_THROWAWAY=1) with Docker and a container lab"]
async fn docker_stopped_while_the_lab_starts_says_so_and_a_start_once_it_is_back_works() {
    if !throwaway() {
        return;
    }
    let (Some(stop), Some(start)) = (knob("CYBERCTF_TEST_RESILIENCE_DOCKER_STOP"), knob("CYBERCTF_TEST_RESILIENCE_DOCKER_START")) else { return };
    on_lab("Docker stopped", async |l: &Run, f: &mut Vec<String>| {
        // Docker goes as soon as the lab's first container exists.
        let stop_it = async {
            let deadline = Instant::now() + Duration::from_secs(600);
            while Instant::now() < deadline {
                if leftovers(&l.id).await.iter().any(|c| c.starts_with("container ")) {
                    return shell(&stop).await.is_ok();
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            false
        };
        let mut said = log(&l.id);
        let (started, stopped) = tokio::join!(l.start(&[], &mut said), stop_it);
        check(stopped, "Docker was stopped while the start ran".into(), f);
        let code = started.as_ref().err().map(crate::error::Error::code);
        check(
            code == Some(crate::error::Code::DockerNotRunning),
            format!("the start says Docker isn't running ({:?})", started.as_ref().err().map(ToString::to_string)),
            f,
        );
        let back = shell(&start).await;
        check(back.is_ok(), format!("Docker is started again ({:?})", back.err()), f);
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline && run("docker", &["info"], None).await.is_err() {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        starts_whole(l, f, "once Docker is back").await;
    })
    .await;
}

/// A fresh copy of the lab in `src`, as the launcher installs one, with its own id
/// (`<prefix>-<n>`) and runner home.
pub(super) fn copy_of(src: &str, prefix: &str) -> Run {
    let id = format!("{prefix}-{}", rand::random::<u16>());
    let dir: PathBuf = install_copy(Path::new(src), &id);
    let home = std::env::temp_dir().join(format!("cyberctf-{id}-home"));
    Run { dir, id, home }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_childs_test_is_where_its_name_says() {
        assert_eq!(super::CHILD_TEST, concat!("integration_tests::resilience::", "the_child_starts_the_lab_it_is_handed"));
        assert!(include_str!("resilience.rs").contains(&format!("async fn {}()", super::CHILD_TEST.rsplit("::").next().unwrap())));
    }
}
