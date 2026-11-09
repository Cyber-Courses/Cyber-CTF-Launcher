//! The subnets Docker's networks already use.

use super::{compose, run_read};

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

/// The IPv4 subnets among those `docker network inspect` printed (IPv6 ones skipped).
pub(super) fn ipv4_subnets(out: &str) -> Vec<String> {
    out.split_whitespace().filter(|s| !s.contains(':')).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

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
        assert_eq!(subnets_in_use("web").await, Vec::<String>::new());
    }
}
