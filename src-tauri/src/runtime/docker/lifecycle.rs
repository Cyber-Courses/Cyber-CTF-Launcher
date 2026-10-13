//! Bringing a lab up and tearing it down.

use std::path::Path;

use super::compose;
use crate::error::{Error, Result};
use crate::exec::{run, run_read};

/// Fails fast with a clear message when the Docker engine isn't reachable, instead of letting
/// `compose up` (or the port/config probes) surface a raw "Cannot connect to the Docker daemon".
/// Timed, so a wedged daemon can't hang the start.
async fn ensure_docker_up() -> Result<()> {
    run_read("docker", &["info", "--format", "{{.ServerVersion}}"], None).await.map(|_| ()).map_err(|e| {
        if crate::exec::docker_denied(&e) {
            Error::Invalid(crate::exec::docker_denied_message())
        } else {
            Error::Invalid(
                "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.".into(),
            )
        }
    })
}

/// A host port is taken if we can't bind it (another lab, or anything else, holds it). Checks
/// every interface a container can publish to: all interfaces and the loopback specifically
/// (a port held only on 127.0.0.1 doesn't fail the 0.0.0.0 bind on its own).
fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_err() || std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

async fn is_running(dir: &Path, project: &str) -> bool {
    match compose::output(dir, project, &["ps", "--format", "json"]).await {
        Ok(out) => compose::parse_ps(&out).iter().any(|e| e.state == "running"),
        Err(_) => false,
    }
}

async fn has_containers(dir: &Path, project: &str) -> bool {
    match compose::output(dir, project, &["ps", "-a", "--format", "json"]).await {
        Ok(out) => !compose::parse_ps(&out).is_empty(),
        Err(_) => false,
    }
}

async fn published_host_ports(dir: &Path, project: &str, env: &[(String, String)]) -> Result<Vec<u16>> {
    let out = compose::output_env(dir, project, &["config", "--format", "json"], env).await?;
    Ok(compose::host_ports_from_config(&out))
}

/// Set (to "default") in a launch's env when the player chose the lab's default ports: each
/// service is published on its own container port (CTFd on 8000) instead of a random free one.
pub const PORTS_ENV: &str = "CYBERCTF_PORTS";

/// Pins the lab's ephemeral host ports (see `compose::PORTS_FILE`), so a shut down / resume keeps
/// the lab at the same address: free loopback ports picked at random, or with `PORTS_ENV` set to
/// "default" each container's own port (a second service on the same port falls back to a random
/// one). Done on a fresh start only: the pins of containers that already exist must stay as they
/// were created. A default port something else holds is caught by `start`'s in-use check.
async fn pin_ports(dir: &Path, project: &str, env: &[(String, String)]) {
    let file = dir.join(compose::PORTS_FILE);
    let _ = std::fs::remove_file(&file);
    let Ok(config) = compose::output_env(dir, project, &["config", "--format", "json"], env).await else { return };
    if let Some(pins) = pick_pins(&config, default_ports(env)) {
        let _ = std::fs::write(&file, pins);
    }
}

/// Whether the player chose the lab's default ports (see [`PORTS_ENV`]).
fn default_ports(env: &[(String, String)]) -> bool {
    env.iter().any(|(k, v)| k == PORTS_ENV && v == "default")
}

/// The pinned-ports override for a lab's Compose `config`: each ephemeral port gets a free
/// loopback port, or with `defaults` its own container port (once per protocol: a second
/// service on the same port and protocol gets a free one; DNS keeps 53 over TCP and UDP).
fn pick_pins(config: &str, defaults: bool) -> Option<String> {
    // Hold each port until all are picked, so the OS doesn't hand out the same one twice.
    let mut held = Vec::new();
    let mut taken = std::collections::HashSet::new();
    let pins = compose::pinned_ports(config, |target, proto| {
        if defaults && target != 0 && taken.insert((target, proto.to_string())) {
            return Some(target);
        }
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = l.local_addr().ok()?.port();
        held.push(l);
        Some(port)
    });
    drop(held);
    pins
}

/// Where the lab's services answer on this machine: (service, container port, protocol, host
/// port) for every published port, the pinned ones included. Empty when Compose can't be read.
pub async fn published(dir: &Path, id: &str, env: &[(String, String)]) -> Vec<(String, u16, String, u16)> {
    compose::output_env(dir, &compose::project(id), &["config", "--format", "json"], env).await.map(|c| compose::published_from_config(&c)).unwrap_or_default()
}

pub async fn start(dir: &Path, id: &str, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    // A stopped engine otherwise surfaces as a raw daemon-connection error much later.
    ensure_docker_up().await?;
    let project = compose::project(id);
    if !has_containers(dir, &project).await {
        pin_ports(dir, &project, env).await;
    }
    // Two labs can't share a host port. Unless this lab is already up (idempotent restart),
    // refuse up front with a clear message instead of a cryptic Docker bind error. With the
    // engine confirmed up, a failure reading the ports is a real Compose-config error worth
    // surfacing now rather than letting `up` fail on it.
    if !is_running(dir, &project).await {
        for port in published_host_ports(dir, &project, env).await? {
            if port_in_use(port) {
                return Err(Error::Invalid(format!(
                    "Host port {port} is already in use on this machine (another lab or app holds it). Stop whatever uses it, or start the lab with random ports."
                )));
            }
        }
    }
    remove_stale_networks(dir, &project, env).await;
    // --wait blocks until containers are healthy; bound it so a container stuck in a failing
    // healthcheck surfaces as a timeout instead of hanging the start indefinitely.
    // Compose's --wait fails when a one-shot job exits (even with 0) unless a running service
    // depends on it: an `init:` on a machine nothing depends on. Isoloom's start plan names
    // those jobs: wait for everything else, then run each attached, failing on its exit code.
    let plan =
        std::fs::read_to_string(crate::runtime::lab::compose_file(dir)).ok().and_then(|c| isoloom_core::generate::start_plan(&c).ok()).unwrap_or_default();
    for step in start_steps(&plan) {
        let step: Vec<&str> = step.iter().map(String::as_str).collect();
        compose::stream(dir, &project, &step, env, &mut log).await?;
    }
    Ok(())
}

/// The Compose commands that start a lab with Isoloom's start `plan`: `up --wait` (on the
/// services to wait for, when some are one-shot jobs), then each job attached, in order.
fn start_steps(plan: &isoloom_core::generate::StartPlan) -> Vec<Vec<String>> {
    let up = ["up", "-d", "--pull", "missing", "--wait", "--wait-timeout", "600"];
    let mut first: Vec<String> = up.iter().map(|s| s.to_string()).collect();
    if plan.jobs.is_empty() {
        return vec![first];
    }
    first.extend(plan.wait.iter().cloned());
    let mut steps = vec![first];
    steps.extend(plan.jobs.iter().map(|job| ["up", "--no-deps", "--exit-code-from", job, job].iter().map(|s| s.to_string()).collect()));
    steps
}

/// Stops the lab's containers, keeping them, their networks and volumes: the lab resumes as it
/// was (`compose start`), no rebuild. The attack box is stopped alongside, so it comes back too.
pub async fn park(dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    let project = compose::project(id);
    // Nothing in the attack box needs a clean shutdown; -t 1 also covers boxes made before
    // they ran with --init (their PID 1 ignores SIGTERM, so a plain stop waited 10 s).
    let _ = run("docker", &["stop", "-t", "1", &crate::runtime::exegol::container(id)], None).await;
    compose::stream(dir, &project, &["stop"], &[], &mut log).await
}

/// Starts the lab's parked containers again, and its attack box with them.
pub async fn resume(dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    ensure_docker_up().await?;
    let project = compose::project(id);
    compose::stream(dir, &project, &["start"], &[], &mut log).await?;
    let _ = run("docker", &["start", &crate::runtime::exegol::container(id)], None).await;
    crate::runtime::exegol::rejoin(id).await;
    Ok(())
}

/// Networks of this lab that its current Compose file no longer defines (left by an older
/// version of the lab), so tools listing the lab's networks don't pick a dead one. Whatever
/// is still plugged into one (the attack box) is unplugged first. Best effort.
async fn remove_stale_networks(dir: &Path, project: &str, env: &[(String, String)]) {
    let Ok(config) = compose::output_env(dir, project, &["config", "--format", "json"], env).await else { return };
    let filter = format!("label=com.docker.compose.project={project}");
    let Ok(listed) = run("docker", &["network", "ls", "--filter", &filter, "--format", "{{.Name}}"], None).await else { return };
    for net in stale_networks(&config, project, &listed) {
        remove_network(&net).await;
    }
}

/// The networks in `listed` (one name a line) that the Compose `config` doesn't define: each
/// network there is its `name:`, else `<project>_<key>`.
fn stale_networks(config: &str, project: &str, listed: &str) -> Vec<String> {
    let wanted: Vec<String> = serde_json::from_str::<serde_json::Value>(config)
        .ok()
        .and_then(|v| {
            v.get("networks")
                .and_then(|n| n.as_object())
                .map(|n| n.iter().map(|(k, v)| v["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("{project}_{k}"))).collect())
        })
        .unwrap_or_default();
    listed.lines().map(str::trim).filter(|n| !n.is_empty() && !wanted.iter().any(|w| w == n)).map(str::to_string).collect()
}

/// Removes containers, networks and volumes: the next start is a clean lab.
pub async fn stop(dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    let project = compose::project(id);
    // The attack box starts with the lab and is plugged into the lab network, but it is not a
    // Compose service of the project (no project label), so `down --remove-orphans` leaves it
    // there and then can't remove the network it still sits on ("Resource is still in use").
    // That leftover network kept the lab's fixed subnet, so a later fresh start failed with
    // "Pool overlaps". Take the attack box down first, so the network is free when Compose
    // removes it.
    let _ = crate::runtime::exegol::stop(id, &mut log).await;
    let down = compose::stream(dir, &project, &["down", "--volumes", "--remove-orphans"], &[], &mut log).await;
    compose::remove_all_containers(&project).await;
    // Belt and braces: whatever network of this lab is still around (something else plugged into
    // it, or Compose gave up), unplug everything from it and remove it, so nothing holds the subnet.
    let filter = format!("label=com.docker.compose.project={project}");
    if let Ok(listed) = run("docker", &["network", "ls", "--filter", &filter, "--format", "{{.Name}}"], None).await {
        for net in listed.lines().map(str::trim).filter(|n| !n.is_empty()) {
            remove_network(net).await;
        }
    }
    // The next start is a new lab: it picks its ports again.
    let _ = std::fs::remove_file(dir.join(compose::PORTS_FILE));
    down
}

/// Unplugs every container from `net` (force) and removes it. Best effort: a network that is
/// already gone, or that something unrelated won't let go of, is left as is.
async fn remove_network(net: &str) {
    if let Ok(attached) = run("docker", &["network", "inspect", "-f", "{{range .Containers}}{{.Name}} {{end}}", net], None).await {
        for c in attached.split_whitespace() {
            let _ = run("docker", &["network", "disconnect", "-f", net, c], None).await;
        }
    }
    let _ = run("docker", &["network", "rm", net], None).await;
}

#[cfg(test)]
mod tests {
    use super::{PORTS_ENV, default_ports, pick_pins, port_in_use, stale_networks, start_steps};
    use isoloom_core::generate::StartPlan;

    #[test]
    fn default_ports_only_when_asked_for() {
        assert!(!default_ports(&[]));
        assert!(!default_ports(&[(PORTS_ENV.into(), "random".into())]));
        assert!(default_ports(&[("X".into(), "y".into()), (PORTS_ENV.into(), "default".into())]));
    }

    #[test]
    fn default_ports_go_to_the_first_service_on_each_port() {
        let config = r#"{"services":{"a":{"ports":[{"target":8000}]},"b":{"ports":[{"target":8000}]}}}"#;
        let yaml = pick_pins(config, true).unwrap();
        assert_eq!(yaml.matches("\"8000:8000/tcp\"").count(), 1, "{yaml}");
        assert_eq!(yaml.matches(":8000/tcp\"").count(), 2, "the second one gets a free port: {yaml}");
        // A port served over both protocols (DNS) keeps its own number for each.
        let dns = r#"{"services":{"dns":{"ports":[{"target":53,"protocol":"tcp"},{"target":53,"protocol":"udp"}]}}}"#;
        let yaml = pick_pins(dns, true).unwrap();
        assert!(yaml.contains("\"53:53/tcp\"") && yaml.contains("\"53:53/udp\""), "{yaml}");
    }

    #[test]
    fn random_ports_are_distinct_free_loopback_ports() {
        let config = r#"{"services":{"a":{"ports":[{"target":80},{"target":443}]}}}"#;
        let yaml = pick_pins(config, false).unwrap();
        let picked: Vec<u16> = yaml.lines().filter_map(|l| l.trim().strip_prefix("- \"")).map(|l| l.split(':').next().unwrap().parse().unwrap()).collect();
        assert_eq!(picked.len(), 2, "{yaml}");
        assert_ne!(picked[0], picked[1]);
        assert!(picked.iter().all(|p| *p != 80 && *p != 443));
        assert_eq!(pick_pins("not json", false), None);
    }

    #[test]
    fn a_held_port_is_in_use() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        assert!(port_in_use(held.local_addr().unwrap().port()));
    }

    #[test]
    fn a_plain_start_waits_for_everything() {
        let steps = start_steps(&StartPlan::default());
        assert_eq!(steps, [["up", "-d", "--pull", "missing", "--wait", "--wait-timeout", "600"]]);
    }

    #[test]
    fn one_shot_jobs_run_attached_after_the_rest_is_up() {
        let plan = StartPlan { wait: vec!["web".into(), "db".into()], jobs: vec!["seed".into(), "claim".into()] };
        let steps = start_steps(&plan);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0][7..], ["web", "db"]);
        assert_eq!(steps[1], ["up", "--no-deps", "--exit-code-from", "seed", "seed"]);
        assert_eq!(steps[2], ["up", "--no-deps", "--exit-code-from", "claim", "claim"]);
    }

    #[test]
    fn stale_networks_are_the_listed_ones_the_config_no_longer_has() {
        let config = r#"{"networks":{"lab":{"name":"cyberctf-x_lab"},"back":{}}}"#;
        let listed = "cyberctf-x_lab\ncyberctf-x_back\ncyberctf-x_old\n\n";
        assert_eq!(stale_networks(config, "cyberctf-x", listed), ["cyberctf-x_old"]);
        // An unreadable config keeps nothing: every listed network is stale.
        assert_eq!(stale_networks("garbage", "p", " a \nb"), ["a", "b"]);
    }
}
