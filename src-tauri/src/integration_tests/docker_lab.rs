//! Docker labs through the Isothrum runner, end to end: generate and start the lab, read its
//! status, run its checks, add the attack box as the lab page does (at the seat the lab declares
//! for the player, else on its first network) and reach the target from a shell in it, shut the lab down and resume it, then pause and
//! resume it (the box following each time, its shell reaching the target again), take the box out, then stop the lab
//! and assert nothing of it is left. Each lab runs from the run manifest Isoloom writes, through
//! the runner — the one lab engine.
//!
//! Opt-in (they need Docker, a lab checkout and the attack-box image):
//!
//! ```text
//! CYBERCTF_TEST_INVOICE_LAB=<invoice-portal-api, in Isoloom's current syntax> \
//! CYBERCTF_TEST_HELLO_LAB=<isoloom checkout>/examples/hello-stack \
//! CYBERCTF_TEST_SEGMENTED_LAB=<isoloom checkout>/examples/segmented \
//! cargo test --lib integration_tests::docker_lab:: -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `CYBERCTF_TEST_BOX_IMAGE` is the attack image of a box the launcher adds (default: Kali's
//! official image).

use std::path::Path;

use super::support::attack_box::{add_box, box_reaches, remove_box};
use super::support::lab::{Run, leftovers};
use super::support::{check, fetch, log, scenario};
use crate::platform::AppDirs;
use crate::platform::exec::run;
use crate::runtime::attack_box::Placed;
use crate::runtime::host::Host;
use crate::runtime::{LabStatus, Park, Place, docker, isothrum};

/// What one lab must show once it is up.
struct Expect {
    /// The env var naming the lab's checkout.
    lab_var: &'static str,
    /// The machine the player attacks.
    target: &'static str,
    /// The lab's networks, by their own names.
    networks: &'static [&'static str],
    /// `<name>=<kind>` services the target declares.
    services: &'static [&'static str],
    /// The target publishes a port on this machine (the lab's local URL).
    published: bool,
    /// The launch's values (an empty one: the lab's development value).
    env: &'static [(&'static str, &'static str)],
}

/// The status the lab page shows: running, on local Docker, every machine up, its networks
/// and the target's services and published port.
fn check_status(s: &LabStatus, e: &Expect, f: &mut Vec<String>) {
    check(s.running, "the lab reports running".into(), f);
    check(matches!(s.place, Some(Place::Container)), "it runs in containers on this machine".into(), f);
    let machines: Vec<String> = s.machines.iter().map(|m| format!("{} ({})", m.name, m.state)).collect();
    println!("machines: {machines:?}");
    check(!s.machines.is_empty(), "it has machines".into(), f);
    for m in s.machines.iter().filter(|m| !m.infra) {
        check(m.state == "running", format!("machine {} is running (state {})", m.name, m.state), f);
    }
    let nets: Vec<String> = s.networks.iter().map(|n| format!("{} {}{}", n.name, n.subnet, if n.internal { " internal" } else { "" })).collect();
    println!("networks: {nets:?}");
    for want in e.networks {
        check(s.networks.iter().any(|n| n.name == *want), format!("network {want} is reported"), f);
    }
    match s.machines.iter().find(|m| m.name == e.target) {
        None => check(false, format!("the target machine {} is listed", e.target), f),
        Some(t) => {
            let services: Vec<String> = t.services.iter().map(|s| format!("{}={}", s.name, s.kind)).collect();
            println!("{} services: {services:?}, ports: {:?}", t.name, t.ports.iter().map(|p| (p.target, p.published)).collect::<Vec<_>>());
            for want in e.services {
                check(services.iter().any(|s| s == want), format!("{} declares {want}", t.name), f);
            }
            if e.published {
                check(t.ports.iter().any(|p| p.published.is_some()), format!("{} publishes a port on this machine", t.name), f);
            }
            check(!t.interfaces.is_empty() && !t.ip.is_empty(), format!("{} has an address on the lab network", t.name), f);
        }
    }
    if e.published {
        check(s.url.as_deref().is_some_and(|u| u.starts_with("http://127.0.0.1:")), format!("the lab has a local URL ({:?})", s.url), f);
    }
}

/// The lab while it is up; every failure is collected so the lab is always stopped after.
async fn exercise(r: &Run, e: &Expect, f: &mut Vec<String>) {
    let mut log = log(&r.id);
    let env: Vec<(String, String)> = e.env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    if let Err(err) = r.start(&env, &mut log).await {
        check(false, format!("the lab starts: {err}"), f);
        return;
    }
    exercise_status(r, e, f).await;
    exercise_checks(r, f).await;
    let attack = attack_box_up(r, e, f).await;
    for mode in [Park::Shutdown, Park::Pause] {
        exercise_park_and_resume(r, mode, f).await;
        if let Some(attack) = &attack {
            box_reaches(attack, e.target, &format!("after the lab resumed from {mode:?}"), f).await;
        }
    }
    if let Some(attack) = &attack {
        remove_box(attack, &AppDirs::new(None, None), &mut log, f).await;
    }
}

/// Adds the attack box as the lab page does (the machine the launcher adds to the lab, at the seat
/// it declares for the player when it declares one, in place of its own access machine), and
/// checks it runs and reaches the target.
async fn attack_box_up(r: &Run, e: &Expect, f: &mut Vec<String>) -> Option<Placed> {
    let run = isothrum::Deployment::docker(&r.home, &r.dir, &r.id).ok()?;
    let placed = Placed { host: Host::here(run), dir: r.dir.clone(), target: crate::runtime::lab::Target::Docker, vm: false };
    let image = std::env::var("CYBERCTF_TEST_BOX_IMAGE").unwrap_or_else(|_| crate::runtime::attack_box::default_image().to_owned());
    add_box(&placed, &AppDirs::new(None, None), &image, &mut log(&r.id), f).await;
    box_reaches(&placed, e.target, "once up", f).await;
    Some(placed)
}

/// Parked in `mode` (Shut down: its containers stopped; Pause: frozen), the lab stops and stays,
/// read as parked that way, each container still there and none running; resumed, it comes back as
/// it was: the same containers, at the same address. The attack box is a machine of the lab, so it
/// parks and resumes with it.
async fn exercise_park_and_resume(r: &Run, mode: Park, f: &mut Vec<String>) {
    let url = r.status().await.ok().and_then(|s| s.url);
    let before = containers(&r.id).await;
    let parked = r.park(mode, &mut log(&r.id)).await;
    check(parked.is_ok(), format!("the lab parks ({mode:?}): {:?}", parked.err()), f);
    let s = r.status().await;
    check(
        s.as_ref().is_ok_and(|s| !s.running && s.parked == Some(mode)),
        format!("parked ({mode:?}), it reads as parked so ({:?})", s.map(|s| (s.running, s.parked))),
        f,
    );
    let held = containers(&r.id).await;
    let frozen = if mode == Park::Pause { "paused" } else { "exited" };
    let kept = before.iter().all(|(id, state)| held.iter().any(|(i, now)| i == id && (state != "running" || now == frozen)));
    check(kept && held.len() == before.len(), format!("parked ({mode:?}), each container is kept, {frozen} if it ran ({before:?} → {held:?})"), f);
    let resumed = r.resume(&mut log(&r.id)).await;
    check(resumed.is_ok(), format!("the lab resumes from {mode:?}: {:?}", resumed.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| s.running && s.url == url), format!("resumed from {mode:?}, it runs at the same address ({url:?})"), f);
    let back = containers(&r.id).await;
    let same = before.iter().all(|(id, state)| state != "running" || back.iter().any(|(i, now)| i == id && now == "running"));
    check(same, format!("resumed from {mode:?}, the same containers run again ({before:?} → {back:?})"), f);
}

/// The lab's containers, by id, with Docker's state of each (`running`, `paused`, `exited`).
async fn containers(id: &str) -> Vec<(String, String)> {
    let label = format!("label=com.docker.compose.project={}", docker::project(id));
    let out = run("docker", &["ps", "--all", "--filter", &label, "--format", "{{.ID}} {{.State}}"], None).await.unwrap_or_default();
    out.lines().filter_map(|l| l.split_once(' ')).map(|(id, state)| (id.to_owned(), state.to_owned())).collect()
}

/// The lab's status, and its published port answering on this machine (as the Open button expects).
async fn exercise_status(r: &Run, e: &Expect, f: &mut Vec<String>) {
    match r.status().await {
        Ok(s) => {
            check_status(&s, e, f);
            if let Some(url) = &s.url {
                let res = fetch(url).await;
                check(res.is_ok(), format!("{url} answers on this machine ({res:?})"), f);
            }
        }
        Err(err) => check(false, format!("the lab's status reads: {err}"), f),
    }
}

/// The lab's own checks pass.
async fn exercise_checks(r: &Run, f: &mut Vec<String>) {
    match r.check().await {
        Ok(c) if !c.available => println!("the lab declares no checks"),
        Ok(c) => {
            for r in &c.results {
                println!(
                    "check {} {} ({}){}",
                    if r.ok { "passed" } else { "FAILED" },
                    r.name,
                    r.from,
                    if r.reason.is_empty() { String::new() } else { format!(": {}", r.reason) }
                );
            }
            check(c.ok && !c.results.is_empty(), format!("the lab's {} checks pass", c.results.len()), f);
            if !c.ok {
                println!("{}", c.output);
            }
        }
        Err(err) => check(false, format!("the lab's checks run: {err}"), f),
    }
}

async fn real_lab(name: &str, e: Expect) {
    // Skipped, naming the knob, without the lab's checkout.
    let Some(src) = super::support::knob(e.lab_var) else { return };
    let r = Run::fresh(Path::new(&src), &format!("ci-{name}"));
    let started = std::time::Instant::now();
    let mut failures = Vec::new();
    scenario(
        &mut failures,
        async |f: &mut Vec<String>| exercise(&r, &e, f).await,
        async |f: &mut Vec<String>| {
            if !f.is_empty() {
                // What the lab said, before stopping it takes its containers (and logs) away.
                let logs = run("docker", &["compose", "-p", &docker::project(&r.id), "logs", "--no-color", "--tail", "200"], None).await;
                println!("compose logs:\n{}", logs.unwrap_or_else(|err| err.to_string()));
            }
            // Stop and remove, as the Stop button does.
            let stopped = r.stop(log(&r.id)).await;
            check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), f);
            let left = leftovers(&r.id).await;
            check(left.is_empty(), format!("nothing of the lab is left: {left:?}"), f);
            let state = crate::testing::runner::run_state(&r.home, &docker::project(&r.id));
            check(state == Some(isothrum_runner::RunState::Absent), format!("the runner records the run as gone ({state:?})"), f);
        },
    )
    .await;
    r.remove_files();
    println!("{name}: {:.0?}", started.elapsed());
    assert!(failures.is_empty(), "{} failed:\n{}", name, failures.join("\n"));
}

/// Isoloom's hello-stack: one network with no internet, a web server publishing its page and a
/// cache seeded by a set-up job, and checks of every kind (http, tcp, exec inside the cache, a
/// script).
fn hello_stack() -> Expect {
    Expect { lab_var: "CYBERCTF_TEST_HELLO_LAB", target: "web", networks: &["app"], services: &["web=web"], published: true, env: &[] }
}

/// A segmented lab: three networks behind a router, reach rules between them, nothing published,
/// checks run from the user's seat (Isoloom's access stand-in) and from inside the segments.
fn segmented() -> Expect {
    Expect { lab_var: "CYBERCTF_TEST_SEGMENTED_LAB", target: "web", networks: &["front", "back", "access"], services: &[], published: false, env: &[] }
}

#[tokio::test]
#[ignore = "needs Docker, Isoloom's hello-stack example and the attack-box image"]
async fn hello_stack_runs_end_to_end() {
    real_lab("hello", hello_stack()).await;
}

#[tokio::test]
#[ignore = "needs Docker, Isoloom's segmented example and the attack-box image"]
async fn segmented_runs_end_to_end() {
    real_lab("segmented", segmented()).await;
}

/// The invoice-portal-api CTF lab: one network, two machines (a web API and its database, which
/// claims the player's evidence in a set-up job), checks of its own (an HTTP probe and a script).
/// `env`: its inputs at launch.
fn invoice_portal(env: &'static [(&'static str, &'static str)]) -> Expect {
    Expect { lab_var: "CYBERCTF_TEST_INVOICE_LAB", target: "web", networks: &["lab"], services: &["portal=web"], published: true, env }
}

#[tokio::test]
#[ignore = "needs Docker, the invoice-portal-api lab (in Isoloom's current syntax) and the attack-box image"]
async fn supplier_portal_runs_end_to_end() {
    // Launched without its inputs: the evidence claim's values are given empty (no launch token).
    real_lab("invoice", invoice_portal(&[])).await;
}
