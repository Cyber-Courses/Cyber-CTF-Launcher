//! Real labs through the launcher's local Docker paths, end to end: generate and start the lab,
//! read its status, start the attack box beside it and reach a target from inside it, run the
//! lab's checks, park and resume it (the attack box following), then stop it and assert nothing
//! of it is left. Each lab runs through the launcher's own Docker code (`launch::start_on_docker`,
//! `status::local_status`, `docker::{check, park, resume}`, `lifecycle::stop_here`) and through
//! the Isothrum runner (`isothrum`, the hidden developer switch's path).
//!
//! Opt-in (they need Docker, a lab checkout and the attack-box image):
//!
//! ```text
//! CYBERCTF_TEST_LAB=<invoice-portal-api, in Isoloom's current syntax> \
//! CYBERCTF_TEST_HELLO_LAB=<isoloom checkout>/examples/hello-stack \
//! CYBERCTF_TEST_SEGMENTED_LAB=<isoloom checkout>/examples/segmented \
//! cargo test real_labs -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `CYBERCTF_TEST_ATTACK_BOX` overrides the attack-box image (default: the launcher's own pinned
//! default, `src-tauri/attack-box-image.json`).

use std::path::{Path, PathBuf};

use super::{LabStatus, Place, Runtime, docker, exegol, isothrum, launch, lifecycle, status};
use crate::error::Result;
use crate::platform::exec::run;

/// Which of the launcher's paths runs the lab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Via {
    /// The launcher's own Docker code.
    Launcher,
    /// The Isothrum runner, from the run manifest Isoloom writes.
    Runner,
}

/// One lab installed for a test, and the path it runs through.
pub(super) struct Run {
    pub(super) dir: PathBuf,
    pub(super) id: String,
    pub(super) via: Via,
    /// The runner's home (its runs under `runs/`), for [`Via::Runner`].
    pub(super) home: PathBuf,
}

impl Run {
    /// Starts the lab with the launch's values `env`.
    pub(super) async fn start(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        match self.via {
            Via::Launcher => launch::start_on_docker(&self.dir, &self.id, env, log).await,
            Via::Runner => launch::start_docker_run(&self.home, &self.dir, &self.id, env, log).await,
        }
    }

    pub(super) async fn status(&self) -> Result<LabStatus> {
        match self.via {
            Via::Launcher => status::local_status(&self.dir, &self.id, Runtime::Docker).await,
            Via::Runner => isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.status().await,
        }
    }

    pub(super) async fn check(&self) -> Result<docker::Check> {
        match self.via {
            Via::Launcher => docker::check(&self.dir, &self.id).await,
            Via::Runner => isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.check().await,
        }
    }

    /// Parks the lab (its containers stopped, kept), as the lab page's Shut down does.
    pub(super) async fn park(&self, log: &mut impl FnMut(String)) -> Result<()> {
        match self.via {
            Via::Launcher => {
                docker::park(&self.dir, &self.id, log).await?;
                super::paths::mark_parked(&self.dir, Some(super::Park::Shutdown))
            }
            Via::Runner => isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.park(super::Park::Shutdown, log).await,
        }
    }

    /// Starts the parked lab again, as it was.
    pub(super) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        match self.via {
            Via::Launcher => {
                docker::resume(&self.dir, &self.id, log).await?;
                super::paths::mark_parked(&self.dir, None)
            }
            Via::Runner => isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.resume(log).await,
        }
    }

    pub(super) async fn stop(&self, log: impl FnMut(String)) -> Result<()> {
        match self.via {
            Via::Launcher => lifecycle::stop_here(&self.dir, &self.id, Runtime::Docker, log).await,
            Via::Runner => lifecycle::stop_docker_run(&self.home, &self.dir, &self.id, log).await,
        }
    }
}

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
    /// Fetched from inside the attack box, by the target's name on the lab network.
    url: &'static str,
    /// The HTTP status it answers with.
    code: &'static str,
    /// Text its body must contain (empty: any).
    body: &'static str,
    /// The target publishes a port on this machine (the lab's local URL).
    published: bool,
    /// The launch's values (an empty one: the lab's development value).
    env: &'static [(&'static str, &'static str)],
}

fn attack_box_image() -> String {
    std::env::var("CYBERCTF_TEST_ATTACK_BOX").ok().filter(|i| !i.is_empty()).unwrap_or_else(|| exegol::default_image().into())
}

/// A fresh copy of the lab, as the launcher installs one: its own folder, nothing generated.
/// Copied in Rust, not with `cp -R`, so the test runs on Windows too (R7).
pub(super) fn install_copy(src: &Path, id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    copy_tree(src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    dir
}

/// `src` copied to `dst`, without its generated outputs, Vagrant's and Git's folders.
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if [".isoloom", ".git", ".vagrant"].iter().any(|skip| name == *skip) {
            continue;
        }
        let (from, to) = (entry.path(), dst.join(&name));
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub(super) fn log(id: &str) -> impl FnMut(String) + use<> {
    let tag = id.to_string();
    move |l: String| println!("[{tag}] {l}")
}

async fn docker_lines(args: &[&str]) -> Vec<String> {
    run("docker", args, None).await.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Everything of the lab Docker still holds: the project's containers (one-off `run`
/// containers included: they carry the project label too) and networks, the attack box and
/// its own network (not Compose's, so found by name).
pub(super) async fn leftovers(id: &str) -> Vec<String> {
    let label = format!("label=com.docker.compose.project={}", docker::project(id));
    let attacker = format!("name=^{}$", exegol::container(id));
    let attack_net = format!("name=^cyberctf-{id}-attack$");
    let mut left = Vec::new();
    for (what, args) in [
        ("container", vec!["ps", "-a", "--filter", label.as_str(), "--format", "{{.Names}}"]),
        ("network", vec!["network", "ls", "--filter", label.as_str(), "--format", "{{.Name}}"]),
        ("attack box", vec!["ps", "-a", "--filter", attacker.as_str(), "--format", "{{.Names}}"]),
        ("attack network", vec!["network", "ls", "--filter", attack_net.as_str(), "--format", "{{.Name}}"]),
    ] {
        left.extend(docker_lines(&args).await.into_iter().map(|n| format!("{what} {n}")));
    }
    left
}

pub(super) fn check(ok: bool, what: String, failures: &mut Vec<String>) {
    if ok {
        println!("ok: {what}");
    } else {
        println!("FAILED: {what}");
        failures.push(what);
    }
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
    exercise_attack_box(&r.id, e, log, f).await;
    exercise_checks(r, f).await;
    exercise_park_and_resume(r, e, f).await;
}

/// Parked, the lab and its attack box stop and stay; resumed, both come back as they were, the
/// lab at the same address and the attack box on its networks.
async fn exercise_park_and_resume(r: &Run, e: &Expect, f: &mut Vec<String>) {
    let url = r.status().await.ok().and_then(|s| s.url);
    let parked = r.park(&mut log(&r.id)).await;
    check(parked.is_ok(), format!("the lab parks ({:?})", parked.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked.is_some()), format!("parked, it reads as parked ({:?})", s.map(|s| (s.running, s.parked))), f);
    check(!exegol::status(&r.id, &attack_box_image()).await.running, "the attack box stopped with the lab".into(), f);
    let resumed = r.resume(&mut log(&r.id)).await;
    check(resumed.is_ok(), format!("the lab resumes ({:?})", resumed.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| s.running && s.url == url), format!("resumed, it runs at the same address ({url:?})"), f);
    exercise_attack_box_reaches(&r.id, e, f).await;
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

/// The attack box starts on the lab's network and reaches the target.
async fn exercise_attack_box(id: &str, e: &Expect, log: impl FnMut(String), f: &mut Vec<String>) {
    let image = attack_box_image();
    if let Err(err) = exegol::start(id, &image, log).await {
        check(false, format!("the attack box starts: {err}"), f);
        return;
    }
    exercise_attack_box_reaches(id, e, f).await;
}

/// The attack box runs on the lab's network and reaches the target.
async fn exercise_attack_box_reaches(id: &str, e: &Expect, f: &mut Vec<String>) {
    let s = exegol::status(id, &attack_box_image()).await;
    check(s.running && !s.ip.is_empty() && !s.lab_network.is_empty(), format!("the attack box runs at {} on {}", s.ip, s.lab_network), f);
    let attacker = exegol::container(id);
    let got = run("docker", &["exec", &attacker, "curl", "-s", "--max-time", "20", "-w", "\n%{http_code}", e.url], None).await;
    match got {
        Ok(out) => {
            let (body, code) = out.trim_end().rsplit_once('\n').unwrap_or(("", out.trim()));
            check(code == e.code, format!("from the attack box, {} answers {} (got {code})", e.url, e.code), f);
            check(
                body.contains(e.body),
                format!("from the attack box, {} returns {:?} (got {:?})", e.url, e.body, body.chars().take(200).collect::<String>()),
                f,
            );
        }
        Err(err) => check(false, format!("curl {} from the attack box: {err}", e.url), f),
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

async fn real_lab(name: &str, via: Via, e: Expect) {
    let src = std::env::var(e.lab_var).unwrap_or_else(|_| panic!("set {} to the lab's checkout", e.lab_var));
    let id = format!("ci-{name}-{}", rand::random::<u16>());
    let r = Run { dir: install_copy(Path::new(&src), &id), home: std::env::temp_dir().join(format!("cyberctf-{id}-home")), id, via };
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
    if via == Via::Runner {
        let record = std::fs::read_to_string(r.home.join("runs").join(docker::project(&r.id)).join("record.json")).unwrap_or_default();
        check(record.contains(r#""state": "absent""#), format!("the runner records the run as gone ({record})"), &mut failures);
    }
    let _ = std::fs::remove_dir_all(&r.dir);
    let _ = std::fs::remove_dir_all(&r.home);
    println!("{name} ({via:?}): {:.0?}", started.elapsed());
    assert!(failures.is_empty(), "{} failed:\n{}", name, failures.join("\n"));
}

/// Isoloom's hello-stack: one network with no internet, a web server publishing its page and a
/// cache seeded by a set-up job, and checks of every kind (http, tcp, exec inside the cache, a
/// script).
fn hello_stack() -> Expect {
    Expect {
        lab_var: "CYBERCTF_TEST_HELLO_LAB",
        target: "web",
        networks: &["app"],
        services: &["web=web"],
        url: "http://web/",
        code: "200",
        body: "hello from isoloom",
        published: true,
        env: &[],
    }
}

/// A segmented lab: three networks behind a router, reach rules between them, nothing published,
/// checks run from the user's seat (Isoloom's access stand-in) and from inside the segments.
fn segmented() -> Expect {
    Expect {
        lab_var: "CYBERCTF_TEST_SEGMENTED_LAB",
        target: "web",
        networks: &["front", "back", "access"],
        services: &[],
        url: "http://web/",
        code: "200",
        body: "web on the front network",
        published: false,
        env: &[],
    }
}

#[tokio::test]
#[ignore = "needs Docker, Isoloom's hello-stack example and the attack-box image"]
async fn real_labs_hello_stack() {
    real_lab("hello", Via::Launcher, hello_stack()).await;
}

#[tokio::test]
#[ignore = "needs Docker, Isoloom's hello-stack example and the attack-box image"]
async fn real_labs_hello_stack_on_the_runner() {
    real_lab("hello", Via::Runner, hello_stack()).await;
}

#[tokio::test]
#[ignore = "needs Docker, Isoloom's segmented example and the attack-box image"]
async fn real_labs_segmented_on_the_runner() {
    real_lab("segmented", Via::Runner, segmented()).await;
}

/// The invoice-portal-api CTF lab: one network, two machines (a web API and its database, which
/// claims the player's evidence in a set-up job), checks of its own (an HTTP probe and a script).
/// `env`: its inputs at launch.
fn invoice_portal(env: &'static [(&'static str, &'static str)]) -> Expect {
    Expect {
        lab_var: "CYBERCTF_TEST_LAB",
        target: "web",
        networks: &["lab"],
        services: &["portal=web"],
        url: "http://web/api/invoices/INV-20507",
        code: "401",
        body: "",
        published: true,
        env,
    }
}

#[tokio::test]
#[ignore = "needs Docker, the invoice-portal-api lab (in Isoloom's current syntax) and the attack-box image"]
async fn real_labs_supplier_portal() {
    // The launcher's own path runs Compose without the manifest: Compose refuses the lab's inputs
    // unset, so they are given, empty (no launch to claim the evidence from: the development value).
    real_lab("invoice", Via::Launcher, invoice_portal(&[("CTF_API_URL", ""), ("CTF_LAUNCH_TOKEN", "")])).await;
}

#[tokio::test]
#[ignore = "needs Docker, the invoice-portal-api lab (in Isoloom's current syntax) and the attack-box image"]
async fn real_labs_supplier_portal_on_the_runner() {
    // Launched without its inputs: the runner delivers each slot's manifest default.
    real_lab("invoice", Via::Runner, invoice_portal(&[])).await;
}

/// The segmented lab through the launcher's own Docker code.
#[tokio::test]
#[ignore = "needs Docker, Isoloom's segmented example and the attack-box image"]
async fn real_labs_segmented() {
    real_lab("segmented", Via::Launcher, segmented()).await;
}
