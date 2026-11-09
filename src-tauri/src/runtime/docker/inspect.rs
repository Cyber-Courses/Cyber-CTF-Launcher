//! Reading a container lab running here: where it is reachable (the URL Open goes to), and the
//! address blocks Docker's networks hold.

use std::collections::HashMap;
use std::path::Path;

use super::compose::{self, PsEntry, Publisher};
use crate::platform::exec::run_read;
use crate::runtime::Service;

mod networks;

pub use networks::subnets_in_use;
#[cfg(test)]
use networks::{ipv4_subnets, networks_of_others};

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
    let repository = crate::platform::exec::image_repository(&e.image).to_ascii_lowercase();
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

/// Where Open goes: a service the lab declares as HTTP (Isoloom's `isoloom.port.*` labels, in
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

/// What `docker inspect` tells about one running container: the services the lab declares in
/// it.
#[derive(Default)]
struct Inspected {
    services: Vec<Service>,
}

/// Each running container's declared services, keyed by container name. `docker compose ps`
/// carries no parseable label map (it joins labels with commas), so we inspect the live
/// containers once. Best effort: an empty map (e.g. inspect failed) just means Open falls back
/// to the first published port.
async fn inspect_containers(dir: &Path, names: &[String]) -> HashMap<String, Inspected> {
    if names.is_empty() {
        return HashMap::new();
    }
    let prefix = compose::context_prefix(dir);
    let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
    args.extend(["inspect", "-f", "{{.Name}}\t{{json .Config.Labels}}"]);
    args.extend(names.iter().map(String::as_str));
    let Ok(out) = run_read("docker", &args, Some(dir)).await else {
        return HashMap::new();
    };
    parse_inspect(&out)
}

/// `docker inspect`'s lines (`<name>\t<labels as JSON>`) as each container's declared services.
fn parse_inspect(out: &str) -> HashMap<String, Inspected> {
    out.lines()
        .filter_map(|line| {
            let (name, labels) = line.split_once('\t')?;
            let labels: HashMap<String, String> = serde_json::from_str(labels).unwrap_or_default();
            Some((name.trim_start_matches('/').to_string(), Inspected { services: declared_services(&labels) }))
        })
        .collect()
}

/// The services a lab declares on a container: Isoloom's `isoloom.port.<name>: "<http|tcp|udp>:<port>"`
/// (or the older `cyberctf.service.<name>: "<kind>:<ports>"`), ports comma-separated and optional.
/// Malformed ports are skipped, never guessed.
fn declared_services(labels: &HashMap<String, String>) -> Vec<Service> {
    let mut services: Vec<Service> = labels
        .iter()
        .filter_map(|(k, v)| {
            let name = k.strip_prefix("isoloom.port.").or_else(|| k.strip_prefix("cyberctf.service."))?.trim();
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

/// Where the lab is reachable on this machine (its first published port), once running.
pub async fn primary_url(dir: &Path, id: &str) -> Option<String> {
    let project = compose::project(id);
    let out = compose::output(dir, &project, &["ps", "--format", "json"]).await.ok()?;
    let entries = compose::parse_ps(&out);
    let names: Vec<String> = entries.iter().filter(|e| e.state == "running").map(|e| e.name.clone()).collect();
    open_url(&entries, &inspect_containers(dir, &names).await)
}

#[cfg(test)]
mod tests;
