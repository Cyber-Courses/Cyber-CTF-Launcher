//! The Docker networks a lab sits on, and the subnets other networks already use.

use super::*;

/// The lab's networks (Compose labels them with the project), with subnet and isolation.
pub(super) async fn lab_networks(id: &str) -> Vec<Network> {
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
