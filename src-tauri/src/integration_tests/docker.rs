//! Docker labs through the Isothrum runner, end to end: generate and start the lab, read its
//! status, run its checks, add the attack box as the lab page does (at the seat the lab declares
//! for the player, else on its first network) and reach the target from a shell in it, park and resume the lab (the box following), take the box out, then stop the lab
//! and assert nothing of it is left. Each lab runs from the run manifest Isoloom writes, through
//! the runner — the one lab engine.
//!
//! Opt-in (they need Docker, a lab checkout and the attack-box image):
//!
//! ```text
//! CYBERCTF_TEST_LAB=<invoice-portal-api, in Isoloom's current syntax> \
//! CYBERCTF_TEST_HELLO_LAB=<isoloom checkout>/examples/hello-stack \
//! CYBERCTF_TEST_SEGMENTED_LAB=<isoloom checkout>/examples/segmented \
//! cargo test --lib integration_tests::docker:: -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `CYBERCTF_TEST_ATTACK_BOX` is the attack image of a box the launcher adds (default: Kali's
//! official image).

use std::path::Path;

use super::support::{Run, add_box, box_reaches, check, install_copy, leftovers, log, remove_box};
use crate::platform::AppDirs;
use crate::platform::exec::run;
use crate::runtime::attack_machine::{Host, Placed};
use crate::runtime::{LabStatus, Place, docker, isothrum};

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
    let machines: Vec<String> = s.machines.iter().map(|m| format!("{} ({}, {})", m.name, m.state, m.image)).collect();
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
                check(t.ports.iter().any(|p| p.published > 0), format!("{} publishes a port on this machine", t.name), f);
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
    let Some(attack) = attack_box_up(r, e, f).await else { return exercise_park_and_resume(r, f).await };
    exercise_park_and_resume(r, f).await;
    box_reaches(&attack, e.target, "after the lab resumed", f).await;
    remove_box(&attack, &AppDirs::new(None, None), &mut log, f).await;
}

/// Adds the attack box as the lab page does (the machine the launcher adds to the lab, at the seat
/// it declares for the player when it declares one, in place of its own access machine), and
/// checks it runs and reaches the target.
async fn attack_box_up(r: &Run, e: &Expect, f: &mut Vec<String>) -> Option<Placed> {
    let run = isothrum::LabRun::docker(&r.home, &r.dir, &r.id).ok()?;
    let placed = Placed { host: Host::Local(run), dir: r.dir.clone(), target: isoloom_spec::Target::Docker, vm: false };
    let image = std::env::var("CYBERCTF_TEST_ATTACK_BOX").unwrap_or_else(|_| crate::runtime::attack_machine::default_image().to_owned());
    add_box(&placed, &AppDirs::new(None, None), &image, &mut log(&r.id), f).await;
    box_reaches(&placed, e.target, "once up", f).await;
    Some(placed)
}

/// Parked, the lab stops and stays; resumed, it comes back as it was, at the same address. The
/// attack box is a machine of the lab, so it parks and resumes with it.
async fn exercise_park_and_resume(r: &Run, f: &mut Vec<String>) {
    let url = r.status().await.ok().and_then(|s| s.url);
    let parked = r.park(&mut log(&r.id)).await;
    check(parked.is_ok(), format!("the lab parks ({:?})", parked.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked.is_some()), format!("parked, it reads as parked ({:?})", s.map(|s| (s.running, s.parked))), f);
    let resumed = r.resume(&mut log(&r.id)).await;
    check(resumed.is_ok(), format!("the lab resumes ({:?})", resumed.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| s.running && s.url == url), format!("resumed, it runs at the same address ({url:?})"), f);
}

/// The lab's status, and its published port answering on this machine (as the Open button expects).
async fn exercise_status(r: &Run, e: &Expect, f: &mut Vec<String>) {
    match r.status().await {
        Ok(s) => {
            check_status(&s, e, f);
            if let Some(url) = &s.url {
                let res = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().get(url).send().await;
                check(res.is_ok(), format!("{url} answers on this machine ({:?})", res.as_ref().map(reqwest::Response::status)), f);
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
    let src = std::env::var(e.lab_var).unwrap_or_else(|_| panic!("set {} to the lab's checkout", e.lab_var));
    let id = format!("ci-{name}-{}", rand::random::<u16>());
    let r = Run { dir: install_copy(Path::new(&src), &id), home: std::env::temp_dir().join(format!("cyberctf-{id}-home")), id };
    let started = std::time::Instant::now();
    let mut failures = Vec::new();
    exercise(&r, &e, &mut failures).await;
    if !failures.is_empty() {
        // What the lab said, before stopping it takes its containers (and logs) away.
        let logs = run("docker", &["compose", "-p", &docker::project(&r.id), "logs", "--no-color", "--tail", "200"], None).await;
        println!("compose logs:\n{}", logs.unwrap_or_else(|err| err.to_string()));
    }
    // Stop and remove, whatever happened above (as the Stop button does).
    let stopped = r.stop(log(&r.id)).await;
    check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), &mut failures);
    let left = leftovers(&r.id).await;
    check(left.is_empty(), format!("nothing of the lab is left: {left:?}"), &mut failures);
    let record = std::fs::read_to_string(r.home.join("runs").join(docker::project(&r.id)).join("record.json")).unwrap_or_default();
    check(record.contains(r#""state": "absent""#), format!("the runner records the run as gone ({record})"), &mut failures);
    let _ = std::fs::remove_dir_all(&r.dir);
    let _ = std::fs::remove_dir_all(&r.home);
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
    Expect { lab_var: "CYBERCTF_TEST_LAB", target: "web", networks: &["lab"], services: &["portal=web"], published: true, env }
}

#[tokio::test]
#[ignore = "needs Docker, the invoice-portal-api lab (in Isoloom's current syntax) and the attack-box image"]
async fn supplier_portal_runs_end_to_end() {
    // Launched without its inputs: the runner delivers each slot's manifest default.
    real_lab("invoice", invoice_portal(&[])).await;
}
