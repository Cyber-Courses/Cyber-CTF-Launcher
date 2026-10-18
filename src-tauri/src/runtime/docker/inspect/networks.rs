//! The Docker networks a lab sits on, and the subnets other networks already use.

use super::*;

/// The names of the lab's Docker networks (Compose labels them with the project). Empty when
/// Docker can't say.
pub async fn lab_network_names(id: &str) -> Vec<String> {
    let filter = format!("label=com.docker.compose.project={}", compose::project(id));
    let out = run_read("docker", &["network", "ls", "--filter", &filter, "--format", "{{.Name}}"], None).await.unwrap_or_default();
    listed_names(&out).into_iter().map(str::to_string).collect()
}

/// The lab's networks, with subnet and isolation.
pub(super) async fn lab_networks(id: &str) -> Vec<Network> {
    let names = lab_network_names(id).await;
    if names.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["network", "inspect", "-f", "{{.Name}}\t{{range .IPAM.Config}}{{.Subnet}} {{end}}\t{{.Internal}}"];
    args.extend(names.iter().map(String::as_str));
    match run_read("docker", &args, None).await {
        Ok(out) => parse_networks(&compose::project(id), &out),
        Err(_) => Vec::new(),
    }
}

/// The IPv4 subnets of every Docker network on this machine, but those of `except` (a lab id's
/// own networks, left from an earlier run): what a new lab's networks must not overlap. Empty
/// when Docker can't say.
pub async fn subnets_in_use(except: &str) -> Vec<String> {
    let Ok(listed) = run_read("docker", &["network", "ls", "--format", "{{.Name}}\t{{.Label \"com.docker.compose.project\"}}"], None).await else {
        return Vec::new();
    };
    let names = networks_of_others(&listed, &compose::project(except));
    if names.is_empty() {
        return Vec::new();
    }
    let mut args = vec!["network", "inspect", "-f", "{{range .IPAM.Config}}{{.Subnet}} {{end}}"];
    args.extend(names);
    run_read("docker", &args, None).await.map(|out| ipv4_subnets(&out)).unwrap_or_default()
}

/// The networks `docker network ls` listed (`<name>\t<compose project>` per line) but those of
/// the Compose project `own`, told by its exact label: a name prefix also matched another lab
/// whose id starts with this one's (`web` and `web-2`).
pub(super) fn networks_of_others<'a>(listed: &'a str, own: &str) -> Vec<&'a str> {
    listed
        .lines()
        .filter_map(|l| {
            let (name, project) = l.split_once('\t').unwrap_or((l, ""));
            let name = name.trim();
            (!name.is_empty() && project.trim() != own).then_some(name)
        })
        .collect()
}

/// The names `docker network ls --format {{.Name}}` printed.
pub(super) fn listed_names(out: &str) -> Vec<&str> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// The IPv4 subnets among those `docker network inspect` printed (IPv6 ones skipped).
pub(super) fn ipv4_subnets(out: &str) -> Vec<String> {
    out.split_whitespace().filter(|s| !s.contains(':')).map(str::to_string).collect()
}

pub(super) fn parse_networks(project: &str, out: &str) -> Vec<Network> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    #[test]
    fn parse_networks_takes_the_ipv4_subnet_strips_the_project_and_sorts() {
        // `<name>\t<subnet(s)>\t<internal>`, as `docker network inspect -f` prints it. An IPv6
        // range alongside the IPv4 one is skipped; the project prefix is stripped from the name;
        // `internal` is true only for the literal "true"; the rows come back sorted by name.
        let out = "cyberctf-web_dmz\tfd00::/64 10.30.0.0/24\ttrue\n\
                   cyberctf-web_default\t172.20.0.0/16\tfalse\n";
        let nets = parse_networks("cyberctf-web", out);
        assert_eq!(nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["default", "dmz"]);
        let dmz = nets.iter().find(|n| n.name == "dmz").unwrap();
        assert_eq!(dmz.subnet, "10.30.0.0/24");
        assert!(dmz.internal);
        let default = nets.iter().find(|n| n.name == "default").unwrap();
        assert_eq!(default.subnet, "172.20.0.0/16");
        assert!(!default.internal);
    }

    #[test]
    fn parse_networks_keeps_an_ipv6_only_network_and_tolerates_missing_columns() {
        // No IPv4 range: fall back to whatever subnet there is (the first). A row with no subnet
        // and no internal column is still a network; a row with no name is dropped entirely.
        let out = "cyberctf-x_v6\tfd00:dead::/48\t\n\
                   cyberctf-x_bare\t\t\n\
                   \t172.18.0.0/16\tfalse\n";
        let nets = parse_networks("cyberctf-x", out);
        assert_eq!(nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["bare", "v6"]);
        assert_eq!(nets.iter().find(|n| n.name == "v6").unwrap().subnet, "fd00:dead::/48");
        let bare = nets.iter().find(|n| n.name == "bare").unwrap();
        assert_eq!(bare.subnet, "");
        assert!(!bare.internal);
        assert!(parse_networks("cyberctf-x", "").is_empty());
    }

    #[tokio::test]
    async fn lab_networks_lists_then_inspects_and_parses() {
        // `network ls` names the lab's networks; `network inspect` gives each one's subnet and
        // isolation, which `parse_networks` turns into the diagram's segments.
        let _tools = Tools::rules(&[
            ("network ls", ok("cyberctf-mini_lab\ncyberctf-mini_wan\n")),
            ("network inspect", ok("cyberctf-mini_lab\t10.10.0.0/24\ttrue\ncyberctf-mini_wan\t10.11.0.0/24\tfalse\n")),
        ]);
        let nets = lab_networks("mini").await;
        assert_eq!(nets.iter().map(|n| (n.name.as_str(), n.internal)).collect::<Vec<_>>(), [("lab", true), ("wan", false)]);
    }

    #[tokio::test]
    async fn lab_networks_is_empty_when_docker_lists_none_or_cannot_say() {
        // No networks listed: no inspect call, no networks.
        let listed_none = Tools::rules(&[("network ls", ok("\n"))]);
        assert!(lab_networks("mini").await.is_empty());
        assert!(!listed_none.called("network inspect"), "nothing to inspect");
        drop(listed_none);
        // Docker can't even list (daemon down): still empty, never a panic.
        let _down = Tools::rules(&[("network", fail("Cannot connect to the Docker daemon"))]);
        assert!(lab_networks("mini").await.is_empty());
    }

    #[tokio::test]
    async fn subnets_in_use_excludes_our_own_and_skips_ipv6() {
        // `network ls` lists every network with its compose project; ours (by exact project) are
        // excluded, then the rest are inspected and only their IPv4 subnets kept.
        let _tools = Tools::rules(&[
            ("network ls", ok("bridge\t\ncyberctf-web_lab\tcyberctf-web\ncyberctf-other_lab\tcyberctf-other\n")),
            ("network inspect", ok("172.17.0.0/16 fd00::/64\n10.9.0.0/24\n")),
        ]);
        let subnets = subnets_in_use("web").await;
        assert_eq!(subnets, ["172.17.0.0/16", "10.9.0.0/24"]);
    }

    #[tokio::test]
    async fn subnets_in_use_is_empty_when_docker_cannot_list() {
        let _down = Tools::rules(&[("network ls", fail("Cannot connect to the Docker daemon"))]);
        assert!(subnets_in_use("web").await.is_empty());
    }
}
