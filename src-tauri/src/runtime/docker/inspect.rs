//! Reading a running lab: its machines, their networks and addresses, the reachable URL.
//! `status()` is a thin composer; each signal (url, networks, interfaces, health) is its
//! own small function so features stop sharing one body.

use std::collections::HashMap;
use std::path::Path;

use super::compose::{self, PsEntry, Publisher};
use crate::error::Result;
use crate::exec::run_read;
use crate::runtime::{Interface, LabStatus, Machine, Network, Port, Service};

mod networks;

#[cfg(test)]
use networks::{ipv4_subnets, listed_names, networks_of_others};
pub use networks::{lab_network_names, subnets_in_use};
use networks::{lab_networks, parse_networks};

/// Whole words of a service's name or image repository that mark it as no web UI: a
/// datastore, or a non-HTTP network service (a daemon's name with or without its `d`).
const NOT_WEB: &[&str] = &[
    "mysql",
    "mariadb",
    "postgres",
    "postgresql",
    "redis",
    "mongo",
    "mongodb",
    "memcached",
    "rabbitmq",
    "elasticsearch",
    "mssql",
    "oracledb",
    "ssh",
    "openssh",
    "ftp",
    "vsftp",
    "proftp",
    "pureftp",
    "rdp",
    "xrdp",
    "vnc",
    "smtp",
    "postfix",
    "telnet",
    "bind9",
    "dnsmasq",
    "samba",
    "openldap",
];

/// Service names that are no web UI only as the whole name: as a word they also name the web
/// front ends of these services (`db-admin`, `dns-server`, `ldap-user-manager`).
const NOT_WEB_SERVICES: &[&str] = &["db", "database", "sftp", "dns", "smb", "ldap"];

/// Oracle's database images (`gvenzl/oracle-xe`, `oracle/database`), told from its web servers
/// (`oracle-weblogic`) by the word next to `oracle`.
const ORACLE_DATABASE: &[&str] = &["xe", "free", "database", "db"];

/// True for services that aren't a web UI (databases, caches, brokers, and non-HTTP network
/// services like SSH/FTP/RDP): the Open button must never point a browser at one. Whole words
/// of the service name and image repository only: substrings took web apps for them
/// (`wordpress` holds "rdp", `novnc` "vnc", `sftpgo` "ftp", `oracle-weblogic` "oracle").
fn is_datastore(e: &PsEntry) -> bool {
    let repository = crate::exec::image_repository(&e.image).to_ascii_lowercase();
    let service = e.service.to_ascii_lowercase();
    let words = |text: &str| -> Vec<String> { text.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect() };
    let (image_words, all_words) = (words(&repository), words(&format!("{service} {repository}")));
    let has = |words: &[String], k: &str| words.iter().any(|w| w == k || w.strip_suffix('d') == Some(k));
    NOT_WEB.iter().any(|k| has(&all_words, k))
        || NOT_WEB_SERVICES.contains(&service.as_str())
        // An image published as a database (`oracle/database`, `container-registry…/database/free`).
        || has(&image_words, "database")
        || (has(&all_words, "oracle") && ORACLE_DATABASE.iter().any(|k| has(&all_words, k)))
}

/// Whether a published port's protocol is TCP (Compose leaves it empty for TCP at times).
fn tcp(p: &Publisher) -> bool {
    p.protocol.is_empty() || p.protocol == "tcp"
}

/// The loopback URL of the lowest of `ports` (published TCP ports, 0 for none).
fn lowest_url(ports: impl Iterator<Item = u16>) -> Option<String> {
    ports.filter(|p| *p > 0).min().map(|port| format!("http://127.0.0.1:{port}"))
}

/// Where Open goes: a service the lab declares as HTTP (Isoloom's `isoloom.service.*` labels, in
/// `declared` by container name) on its published port, else [`first_published_url`].
fn open_url(entries: &[PsEntry], declared: &HashMap<String, Inspected>) -> Option<String> {
    let running = || entries.iter().filter(|e| e.state == "running");
    let web_ports = running().flat_map(|e| {
        let web: Vec<&Service> = declared.get(&e.name).map(|i| i.services.iter().filter(|s| s.kind == "web").collect()).unwrap_or_default();
        e.publishers.iter().filter(move |p| tcp(p) && web.iter().any(|s| s.ports.is_empty() || s.ports.contains(&p.target_port))).map(|p| p.published_port)
    });
    lowest_url(web_ports).or_else(|| first_published_url(entries))
}

/// The loopback URL of a running web service's first published TCP port, i.e. where the
/// lab's target is reachable on this machine. Only running, non-datastore services count,
/// so Open never lands on a database port or a service whose container has exited. None if
/// no web service is up and publishing yet.
fn first_published_url(entries: &[PsEntry]) -> Option<String> {
    lowest_url(entries.iter().filter(|e| e.state == "running" && !is_datastore(e)).flat_map(|e| &e.publishers).filter(|p| tcp(p)).map(|p| p.published_port))
}

/// The lab's own name for a Docker network: Compose prefixes the compose key with the
/// project (`cyberctf-<id>_dmz` -> `dmz`). Other names are returned unchanged.
pub fn short_network(id: &str, name: &str) -> String {
    strip_project(&compose::project(id), name)
}

/// `<project>_<net>` -> `<net>`; other names unchanged.
fn strip_project(project: &str, name: &str) -> String {
    name.strip_prefix(&format!("{project}_")).unwrap_or(name).to_string()
}

/// What `docker inspect` tells about one running container: its interfaces and the
/// services the lab declares in it.
#[derive(Default)]
struct Inspected {
    interfaces: Vec<Interface>,
    services: Vec<Service>,
}

/// Each running container's interfaces (network -> address) and declared services, keyed by
/// container name. `docker compose ps` carries neither addresses nor a parseable label map
/// (it joins labels with commas), so we inspect the live containers once.
/// Best effort: an empty map (e.g. inspect failed) just means the UI shows no IPs.
async fn inspect_containers(dir: &Path, id: &str, names: &[String]) -> HashMap<String, Inspected> {
    if names.is_empty() {
        return HashMap::new();
    }
    let mut args: Vec<&str> =
        vec!["inspect", "-f", "{{.Name}}\t{{range $k, $v := .NetworkSettings.Networks}}{{$k}}={{$v.IPAddress}} {{end}}\t{{json .Config.Labels}}"];
    args.extend(names.iter().map(String::as_str));
    let out = match run_read("docker", &args, Some(dir)).await {
        Ok(out) => out,
        Err(_) => return HashMap::new(),
    };
    parse_inspect(&compose::project(id), &out)
}

fn parse_inspect(project: &str, out: &str) -> HashMap<String, Inspected> {
    out.lines()
        .filter_map(|line| {
            let mut cols = line.splitn(3, '\t');
            let name = cols.next()?;
            let interfaces = cols
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|kv| kv.split_once('='))
                .filter(|(_, ip)| !ip.is_empty())
                .map(|(net, ip)| Interface { network: strip_project(project, net), ip: ip.to_string() })
                .collect();
            let labels: HashMap<String, String> = cols.next().and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default();
            Some((name.trim_start_matches('/').to_string(), Inspected { interfaces, services: declared_services(&labels) }))
        })
        .collect()
}

/// The services a lab declares on a container: Isoloom's `isoloom.service.<name>: "<http|tcp>:<port>"`
/// (or the older `cyberctf.service.<name>: "<kind>:<ports>"`), ports comma-separated and optional.
/// Malformed ports are skipped, never guessed.
fn declared_services(labels: &HashMap<String, String>) -> Vec<Service> {
    let mut services: Vec<Service> = labels
        .iter()
        .filter_map(|(k, v)| {
            let name = k.strip_prefix("isoloom.service.").or_else(|| k.strip_prefix("cyberctf.service."))?.trim();
            let (kind, ports) = v.split_once(':').unwrap_or((v, ""));
            // Isoloom says http or tcp: a web UI, or the service's own name (mysql, ssh...).
            let kind = match (k.starts_with("isoloom."), kind) {
                (true, "http") => "web",
                (true, _) => name,
                (false, k) => k,
            };
            let ports = ports.split(',').filter_map(|p| p.trim().parse::<u16>().ok()).filter(|p| *p > 0).collect();
            (!name.is_empty()).then(|| Service { name: name.to_string(), kind: kind.trim().to_string(), ports })
        })
        .collect();
    services.sort_by(|a, b| a.name.cmp(&b.name));
    services
}

/// A container's TCP ports, once each. Docker lists a published port once per address
/// family (0.0.0.0 and ::), which would otherwise draw every door twice.
fn tcp_ports(publishers: &[Publisher]) -> Vec<Port> {
    let mut seen = std::collections::HashSet::new();
    publishers
        .iter()
        .filter(|p| (p.target_port > 0 || p.published_port > 0) && (p.protocol.is_empty() || p.protocol == "tcp"))
        .filter(|p| seen.insert((p.published_port, p.target_port)))
        .map(|p| Port { published: p.published_port, target: p.target_port })
        .collect()
}

/// The lab counts as running when at least one container is up. Labs have one-shot init services
/// (evidence claim, place-evidence) that exit 0 after their job, so requiring *every* service to be
/// up would read a healthy lab as stopped right after it finishes starting.
fn is_running(entries: &[compose::PsEntry]) -> bool {
    entries.iter().any(|e| e.state == "running")
}

/// Serving containers (those that publish a port) that should be up but aren't: a lab whose web
/// died is degraded, not fine. One-shot init jobs publish nothing, so `serving` never lists them
/// and their normal exit is ignored here.
fn down_serving(entries: &[compose::PsEntry], serving: &[String]) -> Vec<(String, String)> {
    entries.iter().filter(|e| e.state != "running" && serving.iter().any(|s| s == &e.service)).map(|e| (e.service.clone(), e.state.clone())).collect()
}

/// How many containers the lab's project has, running or not (its status lists only the live
/// ones): what tells a parked or cut-off lab with machines left from one with nothing left.
pub async fn containers(dir: &Path, id: &str) -> Result<usize> {
    let out = compose::output(dir, &compose::project(id), &["ps", "--all", "--format", "json"]).await?;
    Ok(compose::parse_ps(&out).iter().filter(|e| !e.one_off()).count())
}

pub async fn status(dir: &Path, id: &str) -> Result<LabStatus> {
    let project = compose::project(id);
    let out = compose::output(dir, &project, &["ps", "--all", "--format", "json"]).await?;
    let entries: Vec<PsEntry> = compose::parse_ps(&out).into_iter().filter(|e| !e.one_off()).collect();
    // Labs have one-shot init services (e.g. evidence, place-evidence) that exit 0 after
    // doing their job, so the lab is "running" when at least one service is up, not when
    // every service is. Only the live services are reported to the UI.
    let running = is_running(&entries);
    let run_names: Vec<String> = entries.iter().filter(|e| e.state == "running").map(|e| e.name.clone()).collect();
    let inspected = inspect_containers(dir, id, &run_names).await;
    let networks = if running { lab_networks(id).await } else { Vec::new() };
    // Serving containers (those that publish a port) that should be up but aren't: a lab
    // whose web died is reported degraded, not fine. One-shot init jobs publish nothing, so
    // their normal exit is ignored. Config is only consulted while the lab is up.
    let serving = if running {
        compose::output(dir, &project, &["config", "--format", "json"]).await.ok().map(|c| compose::serving_services_from_config(&c)).unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(build_status(entries, inspected, networks, &serving))
}

/// What a lab host (a server or cloud VM running the lab's Compose project) reports, gathered
/// over SSH in one go: `docker compose ps`, `docker inspect` of the running containers and
/// `docker network inspect` of the project's networks, in the formats `status` reads locally.
pub struct HostProbe {
    pub ps: String,
    pub inspect: String,
    pub networks: String,
}

/// The shell script that prints a `HostProbe` on the lab host, sections separated by `@@@`.
pub fn host_probe_script(project: &str) -> String {
    let p = crate::runtime::ssh::sh_quote(project);
    format!(
        "docker compose -p {p} ps --all --format json; echo @@@\n\
         ids=$(docker ps -q); [ -z \"$ids\" ] || docker inspect -f '{INSPECT}' $ids; echo @@@\n\
         nets=$(docker network ls -q --filter label=com.docker.compose.project={p}); [ -z \"$nets\" ] || docker network inspect -f '{NETS}' $nets\n"
    )
}

const INSPECT: &str = "{{.Name}}\t{{range $k, $v := .NetworkSettings.Networks}}{{$k}}={{$v.IPAddress}} {{end}}\t{{json .Config.Labels}}";
const NETS: &str = "{{.Name}}\t{{range .IPAM.Config}}{{.Subnet}} {{end}}\t{{.Internal}}";

impl HostProbe {
    pub fn parse(out: &str) -> Option<HostProbe> {
        let mut parts = out.split("@@@");
        Some(HostProbe { ps: parts.next()?.to_string(), inspect: parts.next()?.to_string(), networks: parts.next().unwrap_or_default().to_string() })
    }
}

/// A remote lab's machines and networks from a `HostProbe`, plus where the attack box (the
/// `attacker` container the launcher runs next to the lab) sits: its address and network.
pub fn status_from_host(project: &str, probe: &HostProbe) -> (LabStatus, Option<(String, String)>) {
    let entries: Vec<PsEntry> = compose::parse_ps(&probe.ps).into_iter().filter(|e| !e.one_off()).collect();
    let mut inspected = parse_inspect(project, &probe.inspect);
    let networks = parse_networks(project, &probe.networks);
    let attacker = inspected.remove("attacker").and_then(|a| {
        let lab: Vec<&Interface> = a.interfaces.iter().filter(|i| networks.iter().any(|n| n.name == i.network)).collect();
        lab.first().map(|i| (i.ip.clone(), i.network.clone()))
    });
    let mut status = build_status(entries, inspected, networks, &[]);
    // A remote lab isn't reachable at 127.0.0.1 on this machine.
    status.url = None;
    (status, attacker)
}

/// The lab's status from its containers, their inspection, its networks and which services
/// should be serving.
fn build_status(entries: Vec<PsEntry>, mut inspected: HashMap<String, Inspected>, networks: Vec<Network>, serving: &[String]) -> LabStatus {
    let running = is_running(&entries);
    let url = if running { open_url(&entries, &inspected) } else { None };
    let down: Vec<(String, String)> = down_serving(&entries, serving);
    // The lab's own networks, in order: a container also sits on Docker's own bridges (the
    // default `bridge`, a publish bridge), whose 172.x address isn't the lab address, and
    // `docker inspect` lists them in random order. Keep only lab-network interfaces, in this
    // order, so the diagram shows each machine's real lab address deterministically.
    let lab_order: Vec<&str> = networks.iter().map(|n| n.name.as_str()).collect();
    let mut machines: Vec<Machine> = entries
        .into_iter()
        .filter(|e| e.state == "running")
        .map(|e| {
            let found = inspected.remove(&e.name).unwrap_or_default();
            running_machine(e, found, &lab_order)
        })
        .collect();
    for (service, state) in down {
        if !machines.iter().any(|m| m.name == service) {
            machines.push(down_machine(service, state));
        }
    }
    docker_status(running, machines, networks, url)
}

/// A running container as a machine: its lab interfaces, the address on the first, its TCP
/// ports and declared services.
fn running_machine(e: PsEntry, inspected: Inspected, lab_order: &[&str]) -> Machine {
    let ports = tcp_ports(&e.publishers);
    let Inspected { interfaces, services } = inspected;
    let interfaces = lab_interfaces(interfaces, lab_order);
    let ip = interfaces.first().map(|i| i.ip.clone()).unwrap_or_default();
    // A running container that fails its compose healthcheck is surfaced as unhealthy,
    // so the UI greys it like a dead one instead of showing a broken lab as fine.
    let state = if e.health == "unhealthy" { "unhealthy".to_string() } else { e.state };
    Machine { name: e.service, state, image: e.image, ip, ports, interfaces, services, infra: false }
}

/// A container's interfaces on the lab's own networks, in the lab's network order.
fn lab_interfaces(mut interfaces: Vec<Interface>, lab_order: &[&str]) -> Vec<Interface> {
    // A lab address is never on Docker's own networks; drop them unconditionally so that
    // even when the lab-network probe came back empty (a transient inspect error), the
    // machine IP can't fall back to a 172.x default-bridge address.
    interfaces.retain(|i| !matches!(i.network.as_str(), "bridge" | "host" | "none"));
    if !lab_order.is_empty() {
        interfaces.retain(|i| lab_order.contains(&i.network.as_str()));
        interfaces.sort_by_key(|i| lab_order.iter().position(|n| *n == i.network).unwrap_or(usize::MAX));
    }
    interfaces
}

/// A service that should be serving but has no running container, in the state it is in.
fn down_machine(service: String, state: String) -> Machine {
    Machine { name: service, state, image: String::new(), ip: String::new(), ports: Vec::new(), interfaces: Vec::new(), services: Vec::new(), infra: false }
}

/// A Docker lab's status from its machines, networks and URL.
fn docker_status(running: bool, machines: Vec<Machine>, networks: Vec<Network>, url: Option<String>) -> LabStatus {
    LabStatus {
        running,
        parked: None,
        machines,
        networks,
        url,
        host: None,
        expires_at: None,
        place: None,
        provider: Some("docker".to_string()),
        attacker: None,
        outputs: Vec::new(),
        message: None,
        reap_error: None,
    }
}

/// Where the lab is reachable on this machine (its first published port), once running.
pub async fn primary_url(dir: &Path, id: &str) -> Option<String> {
    let project = compose::project(id);
    let out = compose::output(dir, &project, &["ps", "--format", "json"]).await.ok()?;
    let entries = compose::parse_ps(&out);
    let names: Vec<String> = entries.iter().filter(|e| e.state == "running").map(|e| e.name.clone()).collect();
    open_url(&entries, &inspect_containers(dir, id, &names).await)
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;
