//! Lab containers running on this machine, grouped per lab with their memory.

use std::collections::BTreeMap;

use super::Workload;
use crate::error::Result;
use crate::platform::exec::{run, run_read};

/// `123.4MiB` / `1.2GiB` / `512kB` → bytes (a fraction of a byte dropped), 0 when it isn't a
/// size. Worked out in integers, so a size is exact and never wraps.
fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let (Some(unit), Some((whole, fraction, scale))) = (unit_bytes(unit.trim()), decimal(num.trim())) else { return 0 };
    let bytes = whole.saturating_mul(unit).saturating_add(fraction.saturating_mul(unit) / scale);
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

/// The bytes in one `unit` of `docker stats`.
fn unit_bytes(unit: &str) -> Option<u128> {
    Some(match unit {
        "B" => 1,
        "kB" | "KB" => 1_000,
        "KiB" => 1 << 10,
        "MB" => 1_000_000,
        "MiB" => 1 << 20,
        "GB" => 1_000_000_000,
        "GiB" => 1 << 30,
        "TB" => 1_000_000_000_000,
        "TiB" => 1 << 40,
        _ => return None,
    })
}

/// A non-negative decimal (`1.25`, `.5`, `3.`) as its whole part and its fraction over a power
/// of ten: `(1, 25, 100)`.
fn decimal(num: &str) -> Option<(u128, u128, u128)> {
    let (whole, fraction) = num.split_once('.').unwrap_or((num, ""));
    let whole = whole.strip_prefix('+').unwrap_or(whole);
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return None;
    }
    // Past 20 digits a fraction adds nothing a byte count can hold (and its product stays in range).
    let fraction = &fraction[..fraction.len().min(20)];
    let value = |s: &str| if s.is_empty() { Some(0) } else { s.parse::<u128>().ok() };
    let scale = 10u128.pow(u32::try_from(fraction.len()).ok()?);
    Some((value(whole).unwrap_or(u128::MAX), value(fraction)?, scale))
}

/// Lab id from a container: its compose project (`cyberctf-<id>`) or, for the attack box
/// (`docker run`, no project), its name `cyberctf-<id>-attacker`.
fn lab_of(name: &str, project: &str) -> Option<String> {
    if let Some(id) = project.strip_prefix("cyberctf-") {
        return Some(id.to_string());
    }
    let rest = name.strip_prefix("cyberctf-")?;
    let id = rest.strip_suffix("-attacker").or_else(|| rest.strip_suffix("-exegol")).unwrap_or(rest);
    Some(id.to_string())
}

/// Container ids per lab, from `docker ps` lines of `id\tname\tcompose project`.
fn containers_by_lab(ps: &str) -> BTreeMap<String, Vec<String>> {
    let mut by_lab: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in ps.lines() {
        let mut f = line.split('\t');
        let (Some(cid), Some(name)) = (f.next(), f.next()) else { continue };
        let project = f.next().unwrap_or("");
        if let Some(id) = lab_of(name, project) {
            by_lab.entry(id).or_default().push(cid.to_string());
        }
    }
    by_lab
}

/// Memory per container id, from `docker stats` lines of `id\tused / limit`.
fn memory_by_container(stats: &str) -> BTreeMap<String, u64> {
    stats
        .lines()
        .filter_map(|l| {
            let (id, usage) = l.split_once('\t')?;
            Some((id.to_string(), parse_size(usage.split('/').next().unwrap_or(""))))
        })
        .collect()
}

/// A container's memory: `docker ps` and `docker stats` may shorten ids differently, so either
/// may be a prefix of the other.
fn memory_of(container: &str, mem: &BTreeMap<String, u64>) -> u64 {
    mem.iter().find(|(k, _)| container.starts_with(k.as_str()) || k.starts_with(container)).map_or(0, |(_, v)| *v)
}

pub(super) async fn workloads() -> Vec<Workload> {
    let Ok(ps) =
        run_read("docker", &["ps", "--filter", "name=^cyberctf-", "--format", "{{.ID}}\t{{.Names}}\t{{.Label \"com.docker.compose.project\"}}"], None).await
    else {
        return Vec::new();
    };
    let by_lab = containers_by_lab(&ps);
    if by_lab.is_empty() {
        return Vec::new();
    }
    // Memory per container, in one call.
    let mut args = vec!["stats", "--no-stream", "--format", "{{.ID}}\t{{.MemUsage}}"];
    args.extend(by_lab.values().flatten().map(String::as_str));
    let mem = memory_by_container(&run_read("docker", &args, None).await.unwrap_or_default());
    workloads_from(by_lab, &mem)
}

/// One workload per lab: its containers and their memory summed.
fn workloads_from(by_lab: BTreeMap<String, Vec<String>>, mem: &BTreeMap<String, u64>) -> Vec<Workload> {
    by_lab
        .into_iter()
        .map(|(id, cids)| {
            let mem_bytes = cids.iter().map(|c| memory_of(c, mem)).sum();
            Workload { id, kind: "docker", count: u32::try_from(cids.len()).unwrap_or(u32::MAX), mem_bytes, provider: None }
        })
        .collect()
}

/// The filter selecting exactly this lab's Compose containers (never another lab's).
fn project_filter(id: &str) -> String {
    format!("label=com.docker.compose.project={}", crate::runtime::compose_project(id))
}

/// Removes a lab's containers, its on-demand attack box (a Compose service of the project)
/// included.
pub(super) async fn stop(id: &str) -> Result<()> {
    let project = crate::runtime::compose_project(id);
    let _ = run("docker", &["compose", "-p", &project, "down", "-t", "5", "--volumes", "--remove-orphans"], None).await;
    // Whatever of this exact project is left.
    let left = run("docker", &["ps", "-aq", "--filter", &project_filter(id)], None).await.unwrap_or_default();
    let ids: Vec<&str> = left.split_whitespace().collect();
    if !ids.is_empty() {
        let mut args = vec!["rm", "-f"];
        args.extend(ids);
        run("docker", &args, None).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_a_lab_never_targets_a_lab_whose_id_extends_it() {
        // Lab `a` must not select lab `a-b`'s containers (`cyberctf-a-b-web-1`): the project label
        // is an exact match, never a prefix.
        assert_eq!(project_filter("a"), "label=com.docker.compose.project=cyberctf-a");
    }

    #[test]
    fn parses_docker_sizes() {
        assert_eq!(parse_size("512B"), 512);
        assert_eq!(parse_size("1.5GiB"), 3 * 512 * 1024 * 1024);
        assert_eq!(parse_size("123.4MiB"), 129_394_278);
        assert_eq!((parse_size(".5kB"), parse_size("3.MB"), parse_size("+2B")), (500, 3_000_000, 2));
        for bad in [".", "1.2.3MB", "-1MB", "1.-2MB", "1e3B"] {
            assert_eq!(parse_size(bad), 0, "{bad}");
        }
        assert_eq!(parse_size(" 20MiB "), 20 * 1024 * 1024);
    }

    #[test]
    fn maps_containers_to_labs() {
        assert_eq!(lab_of("cyberctf-sqli-web-1", "cyberctf-sqli").as_deref(), Some("sqli"));
        assert_eq!(lab_of("cyberctf-sqli-attacker", "").as_deref(), Some("sqli"));
        assert_eq!(lab_of("postgres", ""), None);
    }

    #[test]
    fn groups_containers_and_sums_their_memory() {
        let by_lab = containers_by_lab("aaa111\tcyberctf-sqli-web-1\tcyberctf-sqli\nbbb222\tcyberctf-sqli-attacker\t\nccc333\tpostgres\t\n");
        assert_eq!(by_lab.get("sqli").map(Vec::len), Some(2));
        assert_eq!(by_lab.len(), 1);
        let mem = memory_by_container("aaa111\t10MiB / 1GiB\nbbb222000\t1MiB / 1GiB\n");
        assert_eq!(memory_of("aaa111", &mem), 10 * 1024 * 1024);
        // Ids shortened differently still match.
        assert_eq!(memory_of("bbb222", &mem), 1024 * 1024);
        assert_eq!(memory_of("zzz", &mem), 0);
    }

    #[test]
    fn sizes_docker_cannot_report_count_as_zero() {
        // `docker stats` prints `--` for a container it has no figures for (just started, or
        // gone between the two calls); a short or odd line must not be taken for a number.
        assert_eq!(parse_size("--"), 0);
        assert_eq!(parse_size(""), 0);
        assert_eq!(parse_size("0B"), 0);
        assert_eq!(parse_size("1.5kB"), 1500);
        assert_eq!(parse_size("MiB"), 0);
        let mem = memory_by_container("aaa\t-- / --\nbbb\t3MiB / 1GiB\nlonely\n\n");
        assert_eq!(mem.len(), 2);
        assert_eq!(memory_of("aaa", &mem), 0);
        assert_eq!(memory_of("bbb", &mem), 3 * 1024 * 1024);
    }

    #[test]
    fn a_lab_workload_counts_its_containers_and_their_memory() {
        let by_lab = containers_by_lab("aaa\tcyberctf-sqli-web-1\tcyberctf-sqli\nbbb\tcyberctf-sqli-exegol\t\nccc\tcyberctf-xss-web-1\tcyberctf-xss\n");
        let mem = memory_by_container("aaa\t1kB / 1GiB\nbbb\t2KiB / 1GiB\n");
        let w = workloads_from(by_lab, &mem);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].id.as_str(), w[0].kind, w[0].count, w[0].mem_bytes), ("sqli", "docker", 2, 1000 + 2048));
        assert_eq!((w[1].id.as_str(), w[1].mem_bytes), ("xss", 0));
        assert!(w[0].provider.is_none());
        assert_eq!(parse_size("3MB"), 3_000_000);
        assert_eq!(parse_size("1TB"), 1_000_000_000_000);
        assert_eq!(parse_size("1TiB"), 1024u64.pow(4));
        assert_eq!(parse_size("2GB"), 2_000_000_000);
        assert_eq!(parse_size("7 parsecs"), 0);
        assert!(containers_by_lab("lonely-line\n").is_empty());
    }
}
