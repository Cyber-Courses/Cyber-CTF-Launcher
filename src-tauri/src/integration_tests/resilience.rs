//! What a player does that breaks a running or starting lab, on the real Docker: a container
//! removed (`docker rm -f`) or stopped (`docker stop`) while the lab runs, one removed while the
//! start waits for it, the process running the start killed (SIGKILL), a port the lab publishes
//! taken by another process, and (on a throwaway machine only) Docker itself stopped mid-start.
//! Each scenario does the disruption with the tool a player would use, then checks what the
//! launcher says (the lab page's status, the failure's words), that a start again (or the lab
//! page's resume, or its reset: stop then start) brings the whole lab back, and that its stop
//! leaves nothing behind. docs/contributing/testing/resilience.md is the catalogue these come from; the VM family is
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

use std::path::Path;
use std::time::Duration;

use super::support::lab::{Run, every_machine_runs, leftovers};
use super::support::{Wait, check, eventually, fetch, knob, log, scenario, var};
use crate::error::Code;
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

/// How a scenario waits for the first sign of a start it races (a container made): a few times a
/// second, for as long as a start takes.
const SOON: Wait = Wait::secs(600, Duration::from_millis(200));

/// Whether the start of lab `l` has made a container yet.
async fn has_a_container(l: &Run) -> bool {
    leftovers(&l.id).await.iter().any(|c| c.starts_with("container "))
}

/// Whether `resumed` is the refusal of a resume because some of the lab's machines are gone,
/// `machine` among those it names ([`Code::MachinesGone`]). The sentence the player reads for it
/// is the error's own unit test's (`runtime/isothrum/errors`).
pub(super) fn refused_as_gone<T>(resumed: &crate::error::Result<T>, machine: &str) -> bool {
    matches!(resumed.as_ref().map_err(crate::error::Error::code), Err(Code::MachinesGone { machines }) if machines.split(", ").any(|m| m == machine))
}

/// `machine`'s state in the lab's status `s`, as the lab page shows it.
fn state_of(s: &crate::error::Result<LabStatus>, machine: &str) -> Option<String> {
    s.as_ref().ok()?.machines.iter().find(|m| m.name == machine).map(|m| m.state.clone())
}

/// The lab's own machines in `s`, each with its state.
pub(super) fn states(s: &LabStatus) -> Vec<(String, String)> {
    s.machines.iter().filter(|m| !m.infra).map(|m| (m.name.clone(), m.state.clone())).collect()
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
    check(s.as_ref().is_ok_and(every_machine_runs), format!("{when}: every machine runs ({:?})", s.as_ref().map(states)), f);
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
    l.remove_files();
}

/// Runs `body` on a fresh copy of the lab in `CYBERCTF_TEST_RESILIENCE_LAB`, then stops it and
/// checks nothing is left, whatever happened; fails the test on any failed check.
async fn on_lab(name: &str, body: impl AsyncFnOnce(&Run, &mut Vec<String>)) {
    let Some(src) = knob("CYBERCTF_TEST_RESILIENCE_LAB") else { return };
    let l = Run::fresh(Path::new(&src), "resil");
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
        let shown = state_of(&s, &machine);
        check(s.as_ref().is_ok_and(|s| s.running), format!("the lab still reads as running ({:?})", s.as_ref().map(states)), f);
        check(shown.as_deref() == Some("absent"), format!("{machine} is shown gone ({shown:?})"), f);
        // A resume can't bring back what is gone: refused, naming it.
        let resumed = l.resume(&mut log(&l.id)).await;
        check(refused_as_gone(&resumed, &machine), format!("a resume is refused, naming {machine} ({:?})", resumed.err().map(|e| e.to_string())), f);
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
        let shown = state_of(&l.status().await, &machine);
        check(shown.as_deref().is_some_and(|st| st != "running"), format!("{machine} is shown down ({shown:?})"), f);
        let resumed = l.resume(&mut log(&l.id)).await;
        check(resumed.is_ok(), format!("a resume brings it back ({:?})", resumed.err()), f);
        let s = l.status().await;
        check(s.as_ref().is_ok_and(every_machine_runs), format!("every machine runs again ({:?})", s.as_ref().map(states)), f);
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
            let made = eventually(&format!("{machine}'s container"), SOON, async || container_of(&l.id, &machine).await.ok_or("none yet")).await;
            match made {
                Ok(c) => run("docker", &["rm", "-f", &c], None).await.is_ok(),
                Err(_) => false,
            }
        };
        let mut said = log(&l.id);
        let (started, removed) = tokio::join!(l.start(&[], &mut said), remove);
        check(removed, format!("{machine}'s container was removed while the start ran"), f);
        let s = l.status().await;
        if let Err(e) = &started {
            // The start saw it go: it failed, saying so, and the lab isn't read as whole.
            println!("note: the start failed: {e}");
            // Docker's own words, the only thing that says the container went: no code of the
            // launcher's names it (the runner passes the failed step's output on as it is).
            check(e.to_string().contains("No such container"), "the failure says the container went (Docker's words)".into(), f);
            check(!s.as_ref().is_ok_and(every_machine_runs), format!("the lab isn't read as whole ({:?})", s.as_ref().map(states)), f);
        } else {
            // It went after the start's wait: the page shows it gone.
            let shown = state_of(&s, &machine);
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
        let fixed = crate::runtime::launch::Inputs { values: &[], plain: &[], default_ports: true, docker_context: None };
        let first = crate::runtime::launch::start_containers(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
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
        let refused = crate::runtime::launch::start_containers(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
        let code = refused.as_ref().err().map(crate::error::Error::code);
        check(
            matches!(code, Some(crate::error::Code::PortInUse { port: p, .. }) if p.get() == port),
            format!("the start is refused for port {port}, named ({:?})", refused.as_ref().err().map(ToString::to_string)),
            f,
        );
        let left = leftovers(&l.id).await;
        check(left.is_empty(), format!("the refused start left nothing ({left:?})"), f);
        drop(held);
        let started = crate::runtime::launch::start_containers(&l.home, &l.dir, &l.id, fixed, &mut log(&l.id)).await;
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
/// app does, and returns early without it. Its process isn't a scenario's, so it reads this
/// machine itself: on a unit test's empty machine, QEMU's emulator check found no
/// `qemu-system-x86_64` and the start ended before it made anything.
#[tokio::test]
#[ignore = "run by the killed-start scenarios, as their child process"]
async fn the_child_starts_the_lab_it_is_handed() {
    let Some(handed) = var(CHILD) else { return };
    crate::runtime::providers::testing::read_this_machine();
    let [form, home, dir, id, provider] = handed.lines().collect::<Vec<_>>()[..] else { panic!("{CHILD}: {handed}") };
    let (home, dir) = (Path::new(home), Path::new(dir));
    let started = match form {
        "vm" => {
            let provider = serde_json::from_value(serde_json::json!(provider)).expect("a hypervisor");
            let inputs = super::support::lab::vm_inputs();
            let start = crate::runtime::launch::VmStart { provider, inputs: crate::runtime::launch::Inputs::values(&inputs) };
            crate::runtime::launch::start_vms(home, dir, id, start, &mut log(id)).await
        }
        _ => crate::runtime::launch::start_containers(home, dir, id, crate::runtime::launch::Inputs::default(), &mut log(id)).await,
    };
    println!("child: the start ended before it was killed: {started:?}");
}

/// A process, told from a later one given its id by when it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Proc {
    pub(super) pid: u32,
    /// Its start, in seconds since the epoch.
    pub(super) started: u64,
    /// Its command line, to say which it is.
    pub(super) line: String,
}

/// The processes descended from `root` in `procs` (`(process, its parent)`), `root` not among
/// them: what it started that still runs, however deep (Vagrant's shell script, its launcher,
/// then Ruby).
fn descended_from(root: u32, procs: &[(Proc, Option<u32>)]) -> Vec<Proc> {
    let mut found = vec![root];
    let mut at = 0;
    while let Some(&parent) = found.get(at) {
        for (child, _) in procs.iter().filter(|(_, p)| *p == Some(parent)) {
            if !found.contains(&child.pid) {
                found.push(child.pid);
            }
        }
        at += 1;
    }
    procs.iter().filter(|(p, _)| p.pid != root && found.contains(&p.pid)).map(|(p, _)| p.clone()).collect()
}

/// Every process now, with its parent. Threads aren't processes: on Linux sysinfo lists each
/// thread too, its parent the process it runs in and its command line that process's, so this
/// test's own threads (tokio's workers) read as tools it started and left running.
fn processes() -> Vec<(Proc, Option<u32>)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
    sys.processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            let line = p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
            (Proc { pid: p.pid().as_u32(), started: p.start_time(), line }, p.parent().map(sysinfo::Pid::as_u32))
        })
        .collect()
}

/// The processes `root` started that run now (read while it lives: one whose parent died is
/// nobody's descendant any more).
pub(super) fn started_by(root: u32) -> Vec<Proc> {
    descended_from(root, &processes())
}

/// Those of `procs` still running: the same id, started at the same time.
pub(super) fn still_running(procs: &[Proc]) -> Vec<Proc> {
    let now = processes();
    procs.iter().filter(|p| now.iter().any(|(n, _)| n.pid == p.pid && n.started == p.started)).cloned().collect()
}

/// A start of lab `l` in `form` on `provider` run in a child process, killed (SIGKILL; on
/// Windows, terminated) once `begun` says it has made something; then the run read as failed
/// (never ready). The tools the child started, which it left running (each in a process group
/// of its own, out of a killed process's reach), are returned: the start again must end those
/// still running (the runner's ledger of a dead process's tools), see [`ended_by_the_start_again`].
/// Only the child's own descendants count: a VM's hypervisor process names the run too (QEMU's
/// `-name <run>_<machine>`), but is the lab's, not a tool of the start, and runs on.
pub(super) async fn killed_start(l: &Run, form: &str, provider: &str, begun: impl AsyncFn() -> bool, f: &mut Vec<String>) -> Vec<Proc> {
    // Every line has a value: an empty last one would be dropped.
    let provider = if provider.is_empty() { "-" } else { provider };
    let handed = [form, &l.home.to_string_lossy(), &l.dir.to_string_lossy(), &l.id, provider].join("\n");
    let exe = std::env::current_exe().expect("this test binary");
    let mut child =
        match std::process::Command::new(exe).args(["--ignored", "--exact", "--nocapture", "--test-threads=1", CHILD_TEST]).env(CHILD, handed).spawn() {
            Ok(c) => c,
            Err(e) => {
                check(false, format!("the child start runs ({e})"), f);
                return Vec::new();
            }
        };
    // Until its start has made something, or its process ended before it did (nothing to kill).
    let made = eventually("the child's start to make something", Wait::secs(1800, Duration::from_millis(250)), async || {
        if child.try_wait().ok().flatten().is_some() {
            Ok(false)
        } else if begun().await {
            Ok(true)
        } else {
            Err(())
        }
    })
    .await;
    check(made == Ok(true), "the child's start made something before it was killed".into(), f);
    // Read before the kill: once it is dead, its tools are nobody's children.
    let tools = started_by(child.id());
    let killed = child.kill();
    let _ = child.wait();
    check(killed.is_ok(), format!("the child is killed mid-start ({:?})", killed.err()), f);
    let left = still_running(&tools);
    println!("note: tools the killed start left running: {:?}", left.iter().map(|p| format!("{} {}", p.pid, p.line)).collect::<Vec<_>>());
    // The runner: a run whose start never ended is failed, not ready.
    let run = if form == "vm" { isothrum::Deployment::vm(&l.home, &l.dir, &l.id) } else { isothrum::Deployment::docker(&l.home, &l.dir, &l.id) };
    let state = match run {
        Ok(run) => run.run_status().await.map(|s| s.map(|s| s.state)),
        Err(e) => Err(e),
    };
    check(
        matches!(state, Ok(Some(isothrum_runner::RunState::Failed | isothrum_runner::RunState::Absent))),
        format!("the killed start's run reads as failed, not ready ({state:?})"),
        f,
    );
    left
}

/// After the start again: none of the tools the killed start `left` still runs. The runner ends
/// those of a dead process as its next operation begins; one left running would hold what the
/// start again needs (Vagrant's lock on the machine).
pub(super) async fn ended_by_the_start_again(left: &[Proc], f: &mut Vec<String>) {
    // A killed group's members are gone at once; their reaping, a moment later.
    let ended = eventually("the killed start's tools to be gone", Wait::secs(10, Duration::from_millis(500)), async || {
        let still = still_running(left);
        if still.is_empty() { Ok(()) } else { Err(still) }
    })
    .await;
    let still: Vec<String> = ended.err().unwrap_or_default().iter().map(|p| format!("{} {}", p.pid, p.line)).collect();
    check(still.is_empty(), format!("the start again ended the tools the killed one left ({still:?})"), f);
}

#[tokio::test]
#[ignore = "needs Docker and a container lab (CYBERCTF_TEST_RESILIENCE_LAB)"]
async fn a_start_killed_midway_is_failed_and_a_start_again_works() {
    on_lab("killed start", async |l: &Run, f: &mut Vec<String>| {
        let left = killed_start(l, "docker", "", async || has_a_container(l).await, f).await;
        starts_whole(l, f, "started again after the kill").await;
        ended_by_the_start_again(&left, f).await;
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
            let made = eventually("the lab's first container", SOON, async || if has_a_container(l).await { Ok(()) } else { Err(()) }).await;
            made.is_ok() && shell(&stop).await.is_ok()
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
        let answers = eventually("Docker to answer again", Wait::secs(300, Duration::from_secs(2)), async || {
            run("docker", &["info"], None).await.map(drop).map_err(|e| e.to_string())
        })
        .await;
        check(answers.is_ok(), format!("Docker answers again ({:?})", answers.err()), f);
        starts_whole(l, f, "once Docker is back").await;
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::{Proc, descended_from};

    fn proc(pid: u32, line: &str) -> Proc {
        Proc { pid, started: 100, line: line.into() }
    }

    #[test]
    fn a_killed_starts_tools_are_its_descendants_not_the_vm_that_names_the_run() {
        // The test binary (10) ran `vagrant up` (bash, its launcher, then Ruby); libvirt's daemon
        // (1's child) runs the VM, whose QEMU names the run as well.
        let procs = [
            (proc(1, "init"), None),
            (proc(10, "child test"), Some(1)),
            (proc(11, "bash /usr/bin/vagrant up"), Some(10)),
            (proc(12, "/opt/vagrant/bin/vagrant up"), Some(11)),
            (proc(13, "ruby vagrant up"), Some(12)),
            (proc(20, "libvirtd"), Some(1)),
            (proc(21, "qemu-system-x86_64 -name guest=solo-web-abc123_web"), Some(20)),
            (proc(30, "an unrelated sh"), Some(1)),
        ];
        let lines: Vec<String> = descended_from(10, &procs).into_iter().map(|p| p.line).collect();
        assert_eq!(lines, ["bash /usr/bin/vagrant up", "/opt/vagrant/bin/vagrant up", "ruby vagrant up"]);
        assert_eq!(descended_from(30, &procs), []);
    }

    #[test]
    fn a_process_whose_parent_says_itself_is_read_once() {
        let procs = [(proc(5, "root"), None), (proc(6, "looped"), Some(6)), (proc(7, "child"), Some(5))];
        assert_eq!(descended_from(5, &procs), [proc(7, "child")]);
    }

    #[cfg(unix)]
    #[test]
    fn what_this_process_started_runs_until_it_ends_and_a_later_one_with_its_id_is_not_it() {
        use super::{started_by, still_running};
        let mut sleep = std::process::Command::new("sleep").arg("30").spawn().expect("sleep starts");
        let mine = started_by(std::process::id());
        let it: Vec<Proc> = mine.into_iter().filter(|p| p.pid == sleep.id()).collect();
        assert_eq!(it.len(), 1, "the sleep it started is among its descendants");
        assert_eq!(still_running(&it), it);
        let other = Proc { started: it[0].started + 3600, ..it[0].clone() };
        assert_eq!(still_running(&[other]), [], "the same id started at another time is another process");
        sleep.kill().expect("sleep is killed");
        sleep.wait().expect("sleep is reaped");
        assert_eq!(still_running(&it), []);
    }

    /// This process's own threads (tokio's workers, a thread it spawned) are none of what it
    /// started: on Linux sysinfo lists each one, under this process, with its command line.
    #[test]
    fn this_process_threads_are_not_what_it_started() {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || stopped.recv());
        let own = std::env::args().collect::<Vec<_>>().join(" ");
        let mine = super::started_by(std::process::id());
        stop.send(()).expect("the thread waits");
        thread.join().expect("the thread ends").expect("it was told");
        assert!(!mine.iter().any(|p| p.line == own), "a thread of its own read as started by it: {mine:?}");
    }

    #[test]
    fn the_childs_test_is_where_its_name_says() {
        assert_eq!(super::CHILD_TEST, concat!("integration_tests::resilience::", "the_child_starts_the_lab_it_is_handed"));
        assert!(include_str!("resilience.rs").contains(&format!("async fn {}()", super::CHILD_TEST.rsplit("::").next().unwrap())));
    }
}
