//! Reading a running lab: its machines, their networks and addresses, the reachable URL.
//! `status()` is a thin composer; each signal (url, networks, interfaces, health) is its
//! own small function so features stop sharing one body.

use std::collections::HashMap;
use std::path::Path;

use super::compose::{self, PsEntry, Publisher};
use crate::error::Result;
use crate::exec::run_read;
use crate::runtime::{Interface, LabStatus, Machine, Network, Port, Service};

/// True for services that aren't a web UI (databases, caches, brokers, and non-HTTP network
/// services like SSH/FTP/RDP): the Open button must never point a browser at one.
fn is_datastore(e: &PsEntry) -> bool {
    let s = format!("{} {}", e.service, e.image).to_ascii_lowercase();
    [
        "mysql",
        "mariadb",
        "postgres",
        "redis",
        "mongo",
        "memcached",
        "rabbitmq",
        "elastic",
        "mssql",
        "oracle", // datastores
        "sshd",
        "openssh",
        "vsftpd",
        "proftpd",
        "ftp",
        "rdp",
        "xrdp",
        "vnc",
        "telnet",
        "smtp",
        "postfix",
        "bind9",
        "dnsmasq", // non-web network services
    ]
    .iter()
    .any(|k| s.contains(k))
        || matches!(e.service.as_str(), "db" | "database" | "ssh" | "sftp" | "ftp" | "dns" | "smb" | "ldap")
}

/// The loopback URL of a running web service's first published TCP port, i.e. where the
/// lab's target is reachable on this machine. Only running, non-datastore services count,
/// so Open never lands on a database port or a service whose container has exited. None if
/// no web service is up and publishing yet.
fn first_published_url(entries: &[PsEntry]) -> Option<String> {
    entries
        .iter()
        .filter(|e| e.state == "running" && !is_datastore(e))
        .flat_map(|e| &e.publishers)
        .filter(|p| p.published_port > 0 && (p.protocol.is_empty() || p.protocol == "tcp"))
        .map(|p| p.published_port)
        .min()
        .map(|port| format!("http://127.0.0.1:{port}"))
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

/// The lab's networks (Compose labels them with the project), with subnet and isolation.
async fn lab_networks(id: &str) -> Vec<Network> {
    let filter = format!("label=com.docker.compose.project={}", compose::project(id));
    let Ok(names) = run_read("docker", &["network", "ls", "--filter", &filter, "--format", "{{.Name}}"], None).await else {
        return Vec::new();
    };
    let names = listed_names(&names);
    if names.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["network", "inspect", "-f", "{{.Name}}\t{{range .IPAM.Config}}{{.Subnet}} {{end}}\t{{.Internal}}"];
    args.extend(names);
    match run_read("docker", &args, None).await {
        Ok(out) => parse_networks(&compose::project(id), &out),
        Err(_) => Vec::new(),
    }
}

/// The IPv4 subnets of every Docker network on this machine, but those of `except` (a lab id's
/// own networks, left from an earlier run): what a new lab's networks must not overlap. Empty
/// when Docker can't say.
pub async fn subnets_in_use(except: &str) -> Vec<String> {
    let Ok(names) = run_read("docker", &["network", "ls", "--format", "{{.Name}}"], None).await else {
        return Vec::new();
    };
    let own = compose::project(except);
    let names: Vec<&str> = listed_names(&names).into_iter().filter(|n| !n.starts_with(&own)).collect();
    if names.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["network", "inspect", "-f", "{{range .IPAM.Config}}{{.Subnet}} {{end}}"];
    args.extend(names);
    run_read("docker", &args, None).await.map(|out| ipv4_subnets(&out)).unwrap_or_default()
}

/// The names `docker network ls --format {{.Name}}` printed.
fn listed_names(out: &str) -> Vec<&str> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// The IPv4 subnets among those `docker network inspect` printed (IPv6 ones skipped).
fn ipv4_subnets(out: &str) -> Vec<String> {
    out.split_whitespace().filter(|s| !s.contains(':')).map(str::to_string).collect()
}

fn parse_networks(project: &str, out: &str) -> Vec<Network> {
    let mut nets: Vec<Network> = out
        .lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let name = cols.next()?.trim();
            // Docker may list an IPv6 range too; the diagram shows the IPv4 one.
            let subnets: Vec<&str> = cols.next().unwrap_or_default().split_whitespace().collect();
            let subnet = subnets.iter().find(|s| !s.contains(':')).or(subnets.first()).copied().unwrap_or_default();
            let internal = cols.next().is_some_and(|c| c.trim() == "true");
            (!name.is_empty()).then(|| Network { name: strip_project(project, name), subnet: subnet.to_string(), internal })
        })
        .collect();
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    nets
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
    let url = if running { first_published_url(&entries) } else { None };
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
            let ports = tcp_ports(&e.publishers);
            let Inspected { mut interfaces, services } = inspected.remove(&e.name).unwrap_or_default();
            // A lab address is never on Docker's own networks; drop them unconditionally so that
            // even when the lab-network probe came back empty (a transient inspect error), the
            // machine IP can't fall back to a 172.x default-bridge address.
            interfaces.retain(|i| !matches!(i.network.as_str(), "bridge" | "host" | "none"));
            if !lab_order.is_empty() {
                interfaces.retain(|i| lab_order.contains(&i.network.as_str()));
                interfaces.sort_by_key(|i| lab_order.iter().position(|n| *n == i.network).unwrap_or(usize::MAX));
            }
            let ip = interfaces.first().map(|i| i.ip.clone()).unwrap_or_default();
            // A running container that fails its compose healthcheck is surfaced as unhealthy,
            // so the UI greys it like a dead one instead of showing a broken lab as fine.
            let state = if e.health == "unhealthy" { "unhealthy".to_string() } else { e.state };
            Machine { name: e.service, state, image: e.image, ip, ports, interfaces, services, infra: false }
        })
        .collect();
    for (service, state) in down {
        if !machines.iter().any(|m| m.name == service) {
            machines.push(Machine {
                name: service,
                state,
                image: String::new(),
                ip: String::new(),
                ports: Vec::new(),
                interfaces: Vec::new(),
                services: Vec::new(),
                infra: false,
            });
        }
    }
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
    }
}

/// Where the lab is reachable on this machine (its first published port), once running.
pub async fn primary_url(dir: &Path, id: &str) -> Option<String> {
    let project = compose::project(id);
    let out = compose::output(dir, &project, &["ps", "--format", "json"]).await.ok()?;
    first_published_url(&compose::parse_ps(&out))
}

#[cfg(test)]
mod tests {
    use super::compose::parse_ps;
    use super::{
        HostProbe, declared_services, down_serving, first_published_url, is_datastore, is_running, parse_inspect, parse_networks, short_network,
        status_from_host, tcp_ports,
    };
    use super::{build_status, ipv4_subnets, listed_names};

    #[test]
    fn network_listings_skip_blanks_and_ipv6() {
        assert_eq!(listed_names(" bridge \n\ncyberctf-x_lab\n"), ["bridge", "cyberctf-x_lab"]);
        assert_eq!(ipv4_subnets("172.17.0.0/16 fd00::/64\n10.30.0.0/24 \n"), ["172.17.0.0/16", "10.30.0.0/24"]);
    }

    #[test]
    fn a_serving_container_that_died_still_shows_on_the_diagram() {
        let entries =
            parse_ps(r#"[{"Service":"web","State":"exited","Name":"p-web-1"},{"Service":"db","State":"running","Name":"p-db-1","Health":"unhealthy"}]"#);
        let status = build_status(entries, Default::default(), Vec::new(), &["web".into()]);
        assert!(status.running);
        let states: Vec<(&str, &str)> = status.machines.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
        assert_eq!(states, [("db", "unhealthy"), ("web", "exited")]);
        assert_eq!(status.provider.as_deref(), Some("docker"));
    }

    #[test]
    fn a_remote_lab_host_reports_its_containers_and_the_attack_box() {
        // Captured from a Supplier portal API lab on a Proxmox server.
        let probe = HostProbe::parse(include_str!("testdata/remote_probe.txt")).unwrap();
        let (status, attacker) = status_from_host("invoice-portal-api-1", &probe);
        assert!(status.running);
        let names: Vec<(&str, &str)> = status.machines.iter().map(|m| (m.name.as_str(), m.ip.as_str())).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&("web", "10.21.0.31")) && names.contains(&("database", "10.21.0.32")), "{names:?}");
        assert_eq!(status.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str())).collect::<Vec<_>>(), vec![("lab", "10.21.0.0/24")]);
        assert_eq!(attacker, Some(("10.21.0.2".to_string(), "lab".to_string())));
        assert_eq!(status.url, None, "a remote lab isn't on 127.0.0.1");
    }

    // A compose `ps --all` line, as Docker Compose prints it.
    const INIT_EXITED: &str = r#"[
        {"Service":"web","State":"running"},
        {"Service":"database","State":"running"},
        {"Service":"database-init-1","State":"exited"}]"#;

    #[test]
    fn a_lab_is_running_when_its_one_shot_init_has_exited() {
        // Regression: after a start the evidence init exits 0; the lab must still read as running
        // (web + database up), not flip back to a "Start lab" page.
        let entries = parse_ps(INIT_EXITED);
        assert!(is_running(&entries));
        // The exited init publishes nothing, so it is never counted as a down serving container.
        assert_eq!(down_serving(&entries, &["web".into(), "database".into()]), vec![]);
    }

    #[test]
    fn a_lab_with_only_a_finished_init_is_not_running() {
        let entries = parse_ps(r#"[{"Service":"database-init-1","State":"exited"}]"#);
        assert!(!is_running(&entries));
    }

    #[test]
    fn a_serving_container_that_died_is_reported_down_while_others_run() {
        // web died but the database is up: running (something is up) yet degraded, so web is listed.
        let entries = parse_ps(
            r#"[
            {"Service":"web","State":"exited"},
            {"Service":"database","State":"running"},
            {"Service":"database-init-1","State":"exited"}]"#,
        );
        assert!(is_running(&entries));
        assert_eq!(down_serving(&entries, &["web".into(), "database".into()]), vec![("web".to_string(), "exited".to_string())]);
    }

    #[test]
    fn datastores_are_recognised_by_service_or_image() {
        let ps = |svc: &str, image: &str| parse_ps(&format!(r#"[{{"Service":"{svc}","Image":"{image}","State":"running"}}]"#)).pop().unwrap();
        // By well-known image substrings.
        assert!(is_datastore(&ps("x", "mysql:8.0")));
        assert!(is_datastore(&ps("cache", "redis:7")));
        assert!(is_datastore(&ps("store", "mongo:6")));
        assert!(is_datastore(&ps("search", "docker.elastic.co/elasticsearch:8")));
        // By the conventional service names.
        assert!(is_datastore(&ps("db", "custom-image")));
        assert!(is_datastore(&ps("database", "custom-image")));
        // A plain web app is not a datastore.
        assert!(!is_datastore(&ps("web", "invoice_web")));
        assert!(!is_datastore(&ps("app", "nginx:1.27")));
    }

    #[test]
    fn open_url_skips_a_lone_datastore_even_on_a_low_port() {
        // Only a database is up, publishing the low port 22-like case: Open must stay None.
        let out = r#"[{"Service":"db","Image":"postgres:16","State":"running","Publishers":[{"PublishedPort":22,"Protocol":"tcp"}]}]"#;
        assert_eq!(first_published_url(&parse_ps(out)), None);
    }

    #[test]
    fn open_url_ignores_udp_and_unpublished_ports() {
        let out = r#"[{"Service":"web","Image":"app","State":"running","Publishers":[
            {"PublishedPort":53,"Protocol":"udp"},
            {"PublishedPort":0,"Protocol":"tcp"},
            {"PublishedPort":8080,"Protocol":"tcp"}]}]"#;
        // The udp port and the unpublished one are skipped; the tcp web port wins.
        assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:8080"));
    }

    #[test]
    fn short_network_strips_only_this_labs_project_prefix() {
        assert_eq!(short_network("sqli", "cyberctf-sqli_dmz"), "dmz");
        // A different lab's prefix (or none) is left untouched.
        assert_eq!(short_network("sqli", "cyberctf-other_dmz"), "cyberctf-other_dmz");
        assert_eq!(short_network("sqli", "bridge"), "bridge");
    }

    #[test]
    fn declared_services_maps_isoloom_http_to_web_and_keeps_tcp_name() {
        let labels = [
            ("isoloom.service.portal".to_string(), "http:80,443".to_string()),
            ("isoloom.service.shell".to_string(), "tcp:22".to_string()),
            ("com.docker.compose.service".to_string(), "app".to_string()),
        ]
        .into();
        let s = declared_services(&labels);
        let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
        // isoloom http -> "web"; isoloom tcp -> the service's own name; sorted by name.
        assert_eq!(got, vec![("portal", "web", vec![80, 443]), ("shell", "shell", vec![22])]);
    }

    #[test]
    fn declared_services_uses_the_declared_kind_for_cyberctf_labels() {
        let labels = [("cyberctf.service.cache".to_string(), "redis:6379".to_string())].into();
        let s = declared_services(&labels);
        assert_eq!((s[0].name.as_str(), s[0].kind.as_str(), s[0].ports.clone()), ("cache", "redis", vec![6379]));
    }

    #[test]
    fn declared_services_skips_malformed_ports_and_zero() {
        let labels = [("isoloom.service.db".to_string(), "database:abc,0,5432".to_string())].into();
        let s = declared_services(&labels);
        // Only the valid, non-zero port survives; bad ones are dropped, never guessed.
        assert_eq!(s[0].ports, vec![5432]);
    }

    #[test]
    fn networks_fall_back_to_the_ipv6_subnet_when_no_ipv4() {
        // A network reported with only an IPv6 subnet keeps it rather than showing nothing.
        let out = "cyberctf-x_v6only\tfd00::/64 \ttrue\n";
        let n = parse_networks(&super::compose::project("x"), out);
        assert_eq!(n[0].name, "v6only");
        assert_eq!(n[0].subnet, "fd00::/64");
        assert!(n[0].internal);
    }

    #[test]
    fn tcp_ports_keeps_distinct_pairs_and_drops_udp() {
        let out = r#"[{"Service":"x","State":"running","Publishers":[
            {"TargetPort":80,"PublishedPort":8080,"Protocol":"tcp"},
            {"TargetPort":80,"PublishedPort":8080,"Protocol":"tcp"},
            {"TargetPort":443,"PublishedPort":8443,"Protocol":"tcp"},
            {"TargetPort":53,"PublishedPort":5353,"Protocol":"udp"},
            {"TargetPort":0,"PublishedPort":0,"Protocol":"tcp"}]}]"#;
        let ports = tcp_ports(&parse_ps(out)[0].publishers);
        // The duplicate tcp pair is listed once, udp is dropped, and the all-zero entry is dropped.
        assert_eq!(ports.iter().map(|p| (p.published, p.target)).collect::<Vec<_>>(), vec![(8080, 80), (8443, 443)]);
    }

    #[test]
    fn finds_the_first_published_tcp_port() {
        let out = r#"[
          {"Service":"db","State":"running","Publishers":[{"PublishedPort":0,"Protocol":"tcp"}]},
          {"Service":"web","State":"running","Publishers":[{"PublishedPort":8080,"Protocol":"tcp"},{"PublishedPort":9090,"Protocol":"tcp"}]}
        ]"#;
        assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:8080"));
        let none = r#"[{"Service":"web","State":"running","Publishers":[]}]"#;
        assert_eq!(first_published_url(&parse_ps(none)), None);
    }

    #[test]
    fn open_url_prefers_web_and_skips_datastores() {
        // Web port is numerically higher than the DB's: still pick the web service.
        let out = r#"[
          {"Service":"database","Image":"mysql:8.0","State":"running","Publishers":[{"PublishedPort":3207,"Protocol":"tcp"}]},
          {"Service":"web","Image":"invoice_web","State":"running","Publishers":[{"PublishedPort":3206,"Protocol":"tcp"}]}
        ]"#;
        assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:3206"));
        // Web has exited; only the database is up. Don't point Open at the DB.
        let db_only = r#"[{"Service":"database","Image":"mysql:8.0","State":"running","Publishers":[{"PublishedPort":3207,"Protocol":"tcp"}]},
          {"Service":"web","State":"exited","Publishers":[]}]"#;
        assert_eq!(first_published_url(&parse_ps(db_only)), None);
    }

    #[test]
    fn reads_every_interface_with_the_lab_network_name() {
        let out = "/cyberctf-sqli-web-1\tcyberctf-sqli_dmz=172.21.0.3 cyberctf-sqli_internal=172.22.0.2 \n/cyberctf-sqli-db-1\tcyberctf-sqli_internal=172.22.0.3 \n/x\tcyberctf-sqli_dmz= \n";
        let m: std::collections::HashMap<_, _> = parse_inspect(&super::compose::project("sqli"), out).into_iter().map(|(k, v)| (k, v.interfaces)).collect();
        let web: Vec<(&str, &str)> = m["cyberctf-sqli-web-1"].iter().map(|i| (i.network.as_str(), i.ip.as_str())).collect();
        assert_eq!(web, vec![("dmz", "172.21.0.3"), ("internal", "172.22.0.2")]);
        assert_eq!(m["cyberctf-sqli-db-1"][0].network, "internal");
        assert!(m["x"].is_empty(), "a network with no address yet is skipped");
    }

    #[test]
    fn reads_networks_preferring_ipv4_subnets() {
        let out = "cyberctf-sqli_internal\tfd00::/64 172.22.0.0/16 \ttrue\ncyberctf-sqli_default\t172.21.0.0/16 \tfalse\n";
        let n = parse_networks(&super::compose::project("sqli"), out);
        assert_eq!(n.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["default", "internal"]);
        assert_eq!(n[1].subnet, "172.22.0.0/16");
        assert!(n[1].internal && !n[0].internal);
    }

    #[test]
    fn lists_a_port_published_on_ipv4_and_ipv6_once() {
        let out = r#"[{"Service":"db","State":"running","Publishers":[
          {"URL":"0.0.0.0","TargetPort":3207,"PublishedPort":3207,"Protocol":"tcp"},
          {"URL":"::","TargetPort":3207,"PublishedPort":3207,"Protocol":"tcp"},
          {"URL":"","TargetPort":33060,"PublishedPort":0,"Protocol":"tcp"}]}]"#;
        let ports = tcp_ports(&parse_ps(out)[0].publishers);
        assert_eq!(ports.iter().map(|p| (p.published, p.target)).collect::<Vec<_>>(), vec![(3207, 3207), (0, 33060)]);
    }

    #[test]
    fn reads_the_services_a_lab_declares_and_nothing_else() {
        let out = "/cyberctf-x-app-1\tcyberctf-x_default=172.20.0.2 \t{\"cyberctf.service.web\":\"web:80,443\",\"cyberctf.service.ssh\":\"ssh:22\",\"cyberctf.service.jobs\":\"worker\",\"com.docker.compose.service\":\"app\"}\n";
        let m = parse_inspect(&super::compose::project("x"), out);
        let s = &m["cyberctf-x-app-1"].services;
        let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
        assert_eq!(got, vec![("jobs", "worker", vec![]), ("ssh", "ssh", vec![22]), ("web", "web", vec![80, 443])]);
        assert_eq!(m["cyberctf-x-app-1"].interfaces[0].ip, "172.20.0.2");
        // No labels, null labels: no services (never inferred).
        assert!(parse_inspect(&super::compose::project("x"), "/a\t\tnull\n")["a"].services.is_empty());
        assert!(declared_services(&[("cyberctf.service.db".to_string(), "database:abc".to_string())].into()).first().unwrap().ports.is_empty());
    }
}

#[cfg(test)]
mod proptests;
