//! The command line (`cyberctf`, `src/cli*`) drives a real Docker lab, end to end, as a script
//! or a player at a terminal does: `labs install <folder>` imports and starts it, `labs status`
//! reads it up and runs its checks, `labs stop --yes` stops it, and nothing of it is left on
//! Docker. Each command runs in-process through the command line's own entry
//! ([`crate::cli::run`]) with `--json`, on the scenario's app folders, and its answer is read as
//! the JSON lines a script reads. The unit tests (`cli/tests.rs`) drive every command on scripted
//! tools; this is the one place the command line meets real Docker.
//!
//! Opt-in (it needs Docker and a local lab folder whose image is cached or buildable in time):
//!
//! ```text
//! CYBERCTF_TEST_LOCAL_LAB=<isoloom checkout>/examples/hello-stack \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::cli_lab::a_docker_lab_runs_end_to_end_through_the_command_line
//! ```

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use super::support::lab::leftovers;
use super::support::{check, knob, scenario};
use crate::cli::Exit;
use crate::platform::AppDirs;
use crate::testing::test_app::TestApp;

/// What one `cyberctf … --json` said: how it exited, and its last JSON line (the command's
/// result, or its error), which is what a script reads.
#[derive(Debug)]
struct Said {
    exit: Exit,
    last: Value,
    /// How many of the run service's events it printed before its answer (`"type":"event"`).
    events: usize,
}

impl Said {
    /// The result's data, when the command answered one.
    fn data(&self) -> Option<&Value> {
        (self.exit == Exit::Ok && self.last["type"] == "result").then(|| &self.last["data"])
    }
}

/// Runs `cyberctf <args> --json` on `dirs`, off a terminal (as a script runs it): what it said.
async fn cyberctf(dirs: &AppDirs, args: &[&str]) -> Said {
    let line = ["cyberctf"].iter().chain(args).chain(&["--json"]).map(std::ffi::OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = crate::cli::run(line, false, Some(dirs.clone()), &mut out, &mut err).await;
    let out = String::from_utf8_lossy(&out);
    // Events (the run's log) come first, one line each; the answer is the last line.
    for l in out.lines() {
        println!("[cyberctf {}] {l}", args.join(" "));
    }
    let events = out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|l| l["type"] == "event").count();
    let last = out.lines().last().and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| Value::String(String::from_utf8_lossy(&err).into_owned()));
    Said { exit, last, events }
}

/// `labs status --json`'s answer, as far as the scenario reads it.
#[derive(Deserialize, Debug)]
struct StatusAnswer {
    status: Status,
    /// The lab's checks, run when it runs.
    check: Option<LabCheck>,
}

#[derive(Deserialize, Debug)]
struct Status {
    running: bool,
    machines: Vec<Machine>,
}

#[derive(Deserialize, Debug)]
struct Machine {
    name: String,
    state: String,
    #[serde(default)]
    infra: bool,
}

#[derive(Deserialize, Debug)]
struct LabCheck {
    available: bool,
    ok: bool,
    results: Vec<Value>,
}

impl Status {
    /// Whether it runs, every machine of the lab's own (not its plumbing) running.
    fn up(&self) -> bool {
        self.running && !self.machines.is_empty() && self.machines.iter().filter(|m| !m.infra).all(|m| m.state == "running")
    }

    /// Each of the lab's own machines with its state, for a failure's message.
    fn states(&self) -> Vec<(&str, &str)> {
        self.machines.iter().filter(|m| !m.infra).map(|m| (m.name.as_str(), m.state.as_str())).collect()
    }
}

/// `labs status <id>`, read: its answer, or what it said instead.
async fn status(dirs: &AppDirs, id: &str) -> Result<StatusAnswer, String> {
    let said = cyberctf(dirs, &["labs", "status", id]).await;
    let data = said.data().ok_or_else(|| format!("{:?}: {}", said.exit, said.last))?;
    serde_json::from_value(data.clone()).map_err(|e| format!("an answer of another shape ({e}): {data}"))
}

/// Whether `labs list` says lab `id` runs (none: not listed, or no answer).
async fn listed_running(dirs: &AppDirs, id: &str) -> Option<bool> {
    let said = cyberctf(dirs, &["labs", "list"]).await;
    said.data()?.as_array()?.iter().find(|l| l["id"] == id).and_then(|l| l["running"].as_bool())
}

/// `labs install <src>`: the lab imported and started, under the id the launcher gives the folder,
/// the run service's events (its start, its log) streamed as JSON lines before the answer.
async fn installs(dirs: &AppDirs, src: &Path, id: &str, f: &mut Vec<String>) -> bool {
    let said = cyberctf(dirs, &["labs", "install", &src.to_string_lossy()]).await;
    let lab = said.data().map(|d| d["lab"].clone());
    check(lab.as_ref().and_then(Value::as_str) == Some(id), format!("labs install imports and starts it as {id} ({:?}: {})", said.exit, said.last), f);
    check(said.events > 0, format!("labs install streams the run's events before its answer ({} events)", said.events), f);
    lab.is_some()
}

/// `labs status <id>`: it runs, every machine up, and its own checks (which the status runs on a
/// running lab) pass.
async fn reads_up_and_checked(dirs: &AppDirs, id: &str, f: &mut Vec<String>) {
    match status(dirs, id).await {
        Ok(a) => {
            check(a.status.up(), format!("labs status reads it up ({:?})", a.status.states()), f);
            let listed = listed_running(dirs, id).await;
            check(listed == Some(true), format!("labs list says it runs ({listed:?})"), f);
            let checked = a.check.as_ref().is_some_and(|c| c.available && c.ok && !c.results.is_empty());
            check(checked, format!("labs status runs its checks, and they pass ({:?})", a.check), f);
        }
        Err(e) => check(false, format!("labs status answers: {e}"), f),
    }
}

/// `labs stop <id> --yes`, then nothing of it on Docker and its status read as not running.
async fn stops_clean(dirs: &AppDirs, id: &str, f: &mut Vec<String>) {
    let said = cyberctf(dirs, &["labs", "stop", id, "--yes"]).await;
    check(said.data().is_some(), format!("labs stop --yes stops it ({:?}: {})", said.exit, said.last), f);
    let left = leftovers(id).await;
    check(left.is_empty(), format!("nothing of it is left on Docker ({left:?})"), f);
    let gone = status(dirs, id).await;
    check(gone.as_ref().is_ok_and(|a| !a.status.running), format!("labs status reads it stopped ({:?})", gone.map(|a| a.status.running)), f);
    let listed = listed_running(dirs, id).await;
    check(listed == Some(false), format!("labs list says it doesn't run ({listed:?})"), f);
}

#[tokio::test]
#[ignore = "needs Docker and a local lab folder (CYBERCTF_TEST_LOCAL_LAB)"]
async fn a_docker_lab_runs_end_to_end_through_the_command_line() {
    let Some(src) = knob("CYBERCTF_TEST_LOCAL_LAB") else { return };
    let src = std::path::PathBuf::from(src);
    let t = TestApp::scenario();
    let dirs = t.dirs();
    // The id `labs install` gives the folder, as the app's import gives it.
    let id = crate::labs::local_id(&src).expect("a lab folder with an isoloom.yml");
    let mut f = Vec::new();
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            if installs(&dirs, &src, &id, f).await {
                reads_up_and_checked(&dirs, &id, f).await;
            }
        },
        async |f: &mut Vec<String>| stops_clean(&dirs, &id, f).await,
    )
    .await;
    assert!(f.is_empty(), "the lab failed through the command line:\n{}", f.join("\n"));
}
