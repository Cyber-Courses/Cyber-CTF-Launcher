//! Real labs through the launcher's own local Docker path, end to end: generate and start the
//! lab (`launch::start_on_docker`), read its status (`status::local_status`), start the attack
//! box beside it (`exegol::start`) and reach a target from inside it, run the lab's checks
//! (`docker::check`), then stop it (`lifecycle::stop_here`) and assert nothing of it is left.
//!
//! Opt-in (they need Docker, a lab checkout and the attack-box image), run by `labs.yml`:
//!   CYBERCTF_TEST_LAB=<invoice-portal-api checkout> \
//!   CYBERCTF_TEST_SEGMENTED_LAB=<isoloom checkout>/examples/segmented \
//!   ISOLOOM_HOME=$(mktemp -d) cargo test real_labs -- --ignored --nocapture --test-threads=1
//! `CYBERCTF_TEST_ATTACK_BOX` overrides the attack-box image (default: the launcher's own pinned
//! default, `src-tauri/attack-box-image.json`).

use std::path::{Path, PathBuf};

use super::{LabStatus, Place, Runtime, docker, exegol, launch, lifecycle, status};
use crate::exec::run;

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
}

fn attack_box_image() -> String {
    std::env::var("CYBERCTF_TEST_ATTACK_BOX").ok().filter(|i| !i.is_empty()).unwrap_or_else(|| exegol::default_image().into())
}

/// A fresh copy of the lab, as the launcher installs one: its own folder, nothing generated.
fn install_copy(src: &Path, id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    let copied = std::process::Command::new("cp").arg("-R").arg(src).arg(&dir).status().expect("cp");
    assert!(copied.success(), "copy {} to {}", src.display(), dir.display());
    for generated in [".isoloom", ".git"] {
        let _ = std::fs::remove_dir_all(dir.join(generated));
    }
    dir
}

fn log(id: &str) -> impl FnMut(String) + use<> {
    let tag = id.to_string();
    move |l: String| println!("[{tag}] {l}")
}

async fn docker_lines(args: &[&str]) -> Vec<String> {
    run("docker", args, None).await.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Everything of the lab Docker still holds: the project's containers (one-off `run`
/// containers included: they carry the project label too) and networks, the attack box and
/// its own network (not Compose's, so found by name).
async fn leftovers(id: &str) -> Vec<String> {
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

fn check(ok: bool, what: String, failures: &mut Vec<String>) {
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
async fn exercise(dir: &Path, id: &str, e: &Expect, f: &mut Vec<String>) {
    let mut log = log(id);
    if let Err(err) = launch::start_on_docker(dir, id, &[], &mut log).await {
        check(false, format!("the lab starts: {err}"), f);
        return;
    }
    match status::local_status(dir, id, Runtime::Docker).await {
        Ok(s) => {
            check_status(&s, e, f);
            // The published port answers on this machine, as the Open button expects.
            if let Some(url) = &s.url {
                let res = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().get(url).send().await;
                check(res.is_ok(), format!("{url} answers on this machine ({:?})", res.as_ref().map(|r| r.status())), f);
            }
        }
        Err(err) => check(false, format!("the lab's status reads: {err}"), f),
    }

    let image = attack_box_image();
    match exegol::start(id, &image, log).await {
        Ok(()) => {
            let s = exegol::status(id, &image).await;
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
        Err(err) => check(false, format!("the attack box starts: {err}"), f),
    }

    match docker::check(dir, id).await {
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
    let dir = install_copy(Path::new(&src), &id);
    let started = std::time::Instant::now();
    let mut failures = Vec::new();
    exercise(&dir, &id, &e, &mut failures).await;
    if !failures.is_empty() {
        // What the lab said, before stopping it takes its containers (and logs) away.
        let logs = run("docker", &["compose", "-p", &docker::project(&id), "logs", "--no-color", "--tail", "200"], None).await;
        println!("compose logs:\n{}", logs.unwrap_or_else(|err| err.to_string()));
    }
    // Stop and remove, whatever happened above (as the Stop button does).
    let stopped = lifecycle::stop_here(&dir, &id, Runtime::Docker, log(&id)).await;
    check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), &mut failures);
    let left = leftovers(&id).await;
    check(left.is_empty(), format!("nothing of the lab is left: {left:?}"), &mut failures);
    let _ = std::fs::remove_dir_all(&dir);
    println!("{name}: {:.0?}", started.elapsed());
    assert!(failures.is_empty(), "{} failed:\n{}", name, failures.join("\n"));
}

/// One network, two machines (a web API and its database), checks of its own (an HTTP probe
/// and a script on the lab network).
#[tokio::test]
#[ignore]
async fn real_labs_supplier_portal() {
    real_lab(
        "invoice",
        Expect {
            lab_var: "CYBERCTF_TEST_LAB",
            target: "web",
            networks: &["lab"],
            services: &["portal=web"],
            url: "http://web/api/invoices/INV-20507",
            code: "401",
            body: "",
            published: true,
        },
    )
    .await;
}

/// A segmented lab: three networks behind a router, reach rules between them, nothing published,
/// checks run from the user's seat (Isoloom's access stand-in) and from inside the segments.
#[tokio::test]
#[ignore]
async fn real_labs_segmented() {
    real_lab(
        "segmented",
        Expect {
            lab_var: "CYBERCTF_TEST_SEGMENTED_LAB",
            target: "web",
            networks: &["front", "back", "access"],
            services: &[],
            url: "http://web/",
            code: "200",
            body: "web on the front network",
            published: false,
        },
    )
    .await;
}
