//! Talking to the `docker compose` CLI for one lab, and parsing what it prints. Every
//! compose call in the engine goes through here, so the project/file flags live in one place.

use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::exec::{run_env, run_read, stream as exec_stream, stream_stdin as exec_stream_stdin};

// One compose project per lab, so labs never collide and can be cleaned up by name.
pub fn project(id: &str) -> String {
    format!("cyberctf-{id}")
}

/// The host ports picked at the lab's first start, pinned over the ephemeral ones Isoloom's
/// Compose file asks for (Docker picks a new ephemeral port each time a container starts, so a
/// resumed lab moved to another address). In the lab folder, outside the generated files.
pub(super) const PORTS_FILE: &str = ".cyberctf-ports.yml";

/// A `docker compose -p <project> -f <the lab's Compose file> [-f .cyberctf-ports.yml] <rest...>`
/// argv (the file Isoloom generated, under the lab's output folder, and its pinned ports).
/// Without that file (a lab upgraded or re-instanced while its containers run), the project
/// alone: Compose finds its containers by their labels, so `ps`, `stop`, `start` and `down`
/// still reach them, where a missing `-f` made every one of them fail.
fn args(dir: &Path, project: &str, rest: &[&str]) -> Vec<String> {
    let file = crate::runtime::lab::compose_file(dir);
    let mut a = vec!["compose".to_string(), "-p".into(), project.into()];
    if file.is_file() {
        a.extend(["-f".into(), file.display().to_string()]);
        if dir.join(PORTS_FILE).is_file() {
            a.extend(["-f".into(), PORTS_FILE.into()]);
        }
    }
    a.extend(rest.iter().map(|s| s.to_string()));
    a
}

/// Runs a compose subcommand and returns its stdout.
pub(super) async fn output(dir: &Path, project: &str, rest: &[&str]) -> Result<String> {
    let a = args(dir, project, rest);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    run_read("docker", &a, Some(dir)).await
}

/// Runs a compose subcommand with extra environment, and returns its stdout.
pub(super) async fn output_env(dir: &Path, project: &str, rest: &[&str], env: &[(String, String)]) -> Result<String> {
    let a = args(dir, project, rest);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    run_env("docker", &a, Some(dir), env).await
}

/// Like [`stream`], with `stdin` (a file) as the command's standard input: Isoloom's `exec`
/// check runners are piped into the machine (`exec -T <machine> sh -s`). Run as `docker` itself,
/// so it reaches the same engine as every other compose call (see `exec::build`).
pub(super) async fn stream_with_stdin(dir: &Path, project: &str, rest: &[&str], stdin: &Path, log: impl FnMut(String)) -> Result<()> {
    let a = args(dir, project, rest);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    exec_stream_stdin("docker", &a, Some(dir), stdin, log).await
}

/// Runs a compose subcommand, streaming its output line by line.
pub(super) async fn stream(dir: &Path, project: &str, rest: &[&str], env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    let a = args(dir, project, rest);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    exec_stream("docker", &a, Some(dir), env, log).await
}

#[derive(Deserialize)]
pub(super) struct PsEntry {
    #[serde(rename = "Service")]
    pub(super) service: String,
    #[serde(rename = "State")]
    pub(super) state: String,
    /// Compose healthcheck result: "healthy" / "unhealthy" / "starting" / "" (none declared).
    #[serde(rename = "Health", default)]
    pub(super) health: String,
    #[serde(rename = "Image", default)]
    pub(super) image: String,
    #[serde(rename = "Name", default)]
    pub(super) name: String,
    /// `null` from some Compose versions for a container without ports: as none.
    #[serde(rename = "Publishers", default, deserialize_with = "null_as_empty")]
    pub(super) publishers: Vec<Publisher>,
    /// Compose's labels, comma-joined ("com.docker.compose.oneoff=True,...").
    #[serde(rename = "Labels", default)]
    pub(super) labels: String,
}

/// A JSON `null` where a list is expected reads as an empty list.
fn null_as_empty<'de, D: serde::Deserializer<'de>, T: serde::Deserialize<'de>>(d: D) -> std::result::Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

impl PsEntry {
    /// A `docker compose run` container (a check runner, a one-off job): not one of the lab's
    /// machines.
    pub(super) fn one_off(&self) -> bool {
        self.labels.split(',').any(|l| l.trim() == "com.docker.compose.oneoff=True")
    }
}

/// Removes the one-off `run` containers of a project's `service` (a check runner that hung).
pub(super) async fn remove_one_off(project: &str, service: &str) {
    let p = format!("label=com.docker.compose.project={project}");
    let s = format!("label=com.docker.compose.service={service}");
    if let Ok(ids) = crate::exec::run("docker", &["ps", "-aq", "--filter", &p, "--filter", &s, "--filter", "label=com.docker.compose.oneoff=True"], None).await
        && let Some(args) = rm_args(&ids)
    {
        let _ = crate::exec::run("docker", &args, None).await;
    }
}

/// `docker rm -f` of the containers `docker ps -q` listed; None when it listed none.
fn rm_args(ids: &str) -> Option<Vec<&str>> {
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return None;
    }
    let mut args = vec!["rm", "-f"];
    args.extend(ids);
    Some(args)
}

/// Removes every container of a Compose project, one-off `run` containers included (`down`
/// leaves those, so a check runner still going kept a removed lab "running").
pub(super) async fn remove_all_containers(project: &str) {
    let filter = format!("label=com.docker.compose.project={project}");
    if let Ok(ids) = crate::exec::run("docker", &["ps", "-aq", "--filter", &filter], None).await
        && let Some(args) = rm_args(&ids)
    {
        let _ = crate::exec::run("docker", &args, None).await;
    }
}

#[derive(Deserialize)]
pub(super) struct Publisher {
    #[serde(rename = "PublishedPort", default)]
    pub(super) published_port: u16,
    #[serde(rename = "TargetPort", default)]
    pub(super) target_port: u16,
    #[serde(rename = "Protocol", default)]
    pub(super) protocol: String,
}

// `docker compose ps --format json` prints either a JSON array (older Compose)
// or one JSON object per line (Compose >= 2.21).
pub(super) fn parse_ps(out: &str) -> Vec<PsEntry> {
    let trimmed = out.trim();
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).unwrap_or_default();
    }
    trimmed.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Host ports a compose file publishes, from `docker compose config` (env resolved), each with
/// its protocol (`tcp` or `udp`, Compose's; `tcp` when it names none).
pub(super) fn host_ports_from_config(json: &str) -> Vec<(u16, String)> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut ports = Vec::new();
    if let Some(services) = v.get("services").and_then(|s| s.as_object()) {
        for svc in services.values() {
            let Some(arr) = svc.get("ports").and_then(|p| p.as_array()) else { continue };
            for p in arr {
                let published = p.get("published").and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
                if let Some(n) = published
                    && n > 0
                    && n <= u16::MAX as u64
                {
                    let proto = p.get("protocol").and_then(|x| x.as_str()).filter(|x| !x.is_empty()).unwrap_or("tcp");
                    ports.push((n as u16, proto.to_string()));
                }
            }
        }
    }
    ports
}

/// Every published port as (service, container port, protocol, host port), from `docker compose
/// config` (env resolved, pinned ports applied): where each of the lab's services answers on this
/// machine. The protocol is `tcp` or `udp` (Compose's, `tcp` when it names none).
pub(super) fn published_from_config(json: &str) -> Vec<(String, u16, String, u16)> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let num =
        |x: Option<&serde_json::Value>| x.and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).and_then(|n| u16::try_from(n).ok());
    let mut out = Vec::new();
    for (name, svc) in v.get("services").and_then(|s| s.as_object()).into_iter().flatten() {
        for p in svc.get("ports").and_then(|p| p.as_array()).into_iter().flatten() {
            if let (Some(target), Some(host)) = (num(p.get("target")), num(p.get("published")))
                && host > 0
            {
                let proto = p.get("protocol").and_then(|x| x.as_str()).filter(|x| !x.is_empty()).unwrap_or("tcp");
                out.push((name.clone(), target, proto.to_string(), host));
            }
        }
    }
    out
}

/// Service names a compose file publishes a host port for, from `docker compose config`.
/// These are the lab's serving containers (web/app), as opposed to one-shot init jobs.
pub(super) fn serving_services_from_config(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let mut names = Vec::new();
    if let Some(services) = v.get("services").and_then(|s| s.as_object()) {
        for (name, svc) in services {
            if svc.get("ports").and_then(|p| p.as_array()).is_some_and(|a| !a.is_empty()) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// A double-quoted YAML scalar. JSON's quoting does most of it, but YAML also reads NEL, LS and
/// PS (U+0085, U+2028, U+2029) as line breaks, so every non-ASCII character is escaped too.
fn yaml_str(s: &str) -> String {
    let mut out = String::new();
    for c in serde_json::Value::from(s).to_string().chars() {
        match c as u32 {
            n if n < 0x80 => out.push(c),
            n if n <= 0xFFFF => out.push_str(&format!("\\u{n:04x}")),
            n => out.push_str(&format!("\\U{n:08x}")),
        }
    }
    out
}

/// A Compose override that gives every port published on an ephemeral host port (none set in
/// `docker compose config` JSON) a fixed one from `pick` (given the container port and its
/// protocol, `tcp` or `udp`), so it survives stop / start. A service's
/// whole `ports` list is replaced (`!override`: Compose would otherwise add the entries). None
/// when nothing is ephemeral.
pub(super) fn pinned_ports(json: &str, mut pick: impl FnMut(u16, &str) -> Option<u16>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut out = String::new();
    for (name, svc) in v.get("services")?.as_object()? {
        let Some(ports) = svc.get("ports").and_then(|p| p.as_array()) else { continue };
        let ephemeral = |p: &serde_json::Value| {
            p.get("published")
                .and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string())))
                .is_none_or(|s| s.is_empty() || s == "0")
        };
        if !ports.iter().any(ephemeral) {
            continue;
        }
        let mut lines = Vec::new();
        for p in ports {
            let target = p.get("target").and_then(|t| t.as_u64())?;
            let proto = p.get("protocol").and_then(|i| i.as_str()).unwrap_or("tcp");
            let published = if ephemeral(p) {
                pick(u16::try_from(target).ok()?, proto)?.to_string()
            } else {
                p["published"].as_str().map(str::to_string).unwrap_or_else(|| p["published"].to_string())
            };
            // An IPv6 address goes in brackets, or its colons read as the port separators.
            let ip = p
                .get("host_ip")
                .and_then(|i| i.as_str())
                .filter(|i| !i.is_empty())
                .map(|i| if i.contains(':') { format!("[{i}]:") } else { format!("{i}:") })
                .unwrap_or_default();
            // A JSON string is a valid YAML scalar: a quote or newline in a value can't break the file.
            lines.push(format!("      - {}\n", yaml_str(&format!("{ip}{published}:{target}/{proto}"))));
        }
        out.push_str(&format!("  {}:\n    ports: !override\n{}", yaml_str(name), lines.concat()));
    }
    (!out.is_empty()).then(|| format!("# Written by Cyber CTF at the lab's first start: its host ports, kept across resumes.\nservices:\n{out}"))
}

#[cfg(test)]
mod tests {
    use super::{host_ports_from_config, parse_ps, pinned_ports, project, published_from_config, serving_services_from_config};

    #[test]
    fn unicode_line_breaks_in_names_stay_inside_their_scalar() {
        for name in ["\u{85}", "a\u{2028}b", "\u{2029}", "caf\u{e9} \u{1f600}"] {
            let json = serde_json::json!({"services": {name: {"ports": [{"target": 80, "protocol": "tcp"}]}}}).to_string();
            let yaml = pinned_ports(&json, |_, _| Some(40000)).unwrap();
            let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap_or_else(|e| panic!("{e}\n{yaml}"));
            assert!(doc["services"].as_mapping().unwrap().contains_key(name), "{name:?} in {yaml}");
        }
    }

    #[test]
    fn pins_ephemeral_ports_and_keeps_fixed_ones() {
        let json = r#"{"services":{"web":{"ports":[{"host_ip":"127.0.0.1","target":80,"protocol":"tcp"},{"host_ip":"127.0.0.1","target":443,"published":"8443","protocol":"tcp"}]},"db":{},"dns":{"ports":[{"target":53,"published":"","protocol":"udp"}]}}}"#;
        let mut next = 40000;
        let yaml = pinned_ports(json, |_, _| {
            next += 1;
            Some(next)
        })
        .unwrap();
        // Each ephemeral port gets its own pick, whatever order the services come in.
        let (web, dns) = if yaml.find("\"web\"") < yaml.find("\"dns\"") { (40001, 40002) } else { (40002, 40001) };
        assert!(yaml.contains(&format!("  \"dns\":\n    ports: !override\n      - \"{dns}:53/udp\"\n")), "{yaml}");
        assert!(
            yaml.contains(&format!("  \"web\":\n    ports: !override\n      - \"127.0.0.1:{web}:80/tcp\"\n      - \"127.0.0.1:8443:443/tcp\"\n")),
            "{yaml}"
        );
        assert!(!yaml.contains("db"));
        // Nothing ephemeral, or no free port: no override.
        assert!(pinned_ports(r#"{"services":{"web":{"ports":[{"target":80,"published":"8080"}]}}}"#, |_, _| Some(1)).is_none());
        assert!(pinned_ports(r#"{"services":{"web":{"ports":[{"target":80}]}}}"#, |_, _| None).is_none());
    }

    #[test]
    fn the_pick_sees_each_container_port() {
        let json = r#"{"services":{"a":{"ports":[{"target":8000}]},"b":{"ports":[{"target":3000,"protocol":"tcp"}]}}}"#;
        let yaml = pinned_ports(json, |t, _| Some(t)).unwrap();
        assert!(yaml.contains("\"8000:8000/tcp\"") && yaml.contains("\"3000:3000/tcp\""), "{yaml}");
    }

    #[test]
    fn lists_published_ports_by_service() {
        let json = r#"{"services":{"web":{"ports":[{"target":80,"published":"46709"},{"target":443}]},"db":{},"dns":{"ports":[{"target":53,"published":"40053","protocol":"udp"}]}}}"#;
        let mut found = published_from_config(json);
        found.sort();
        assert_eq!(found, vec![("dns".to_string(), 53, "udp".to_string(), 40053), ("web".to_string(), 80, "tcp".to_string(), 46709)]);
        assert!(published_from_config("not json").is_empty());
    }

    #[test]
    fn reads_published_host_ports_from_config() {
        let json = r#"{"services":{"web":{"ports":[{"published":"3206","target":3206}]},"db":{"ports":[{"published":3207,"target":3207}]},"dns":{"ports":[{"published":"53","target":53,"protocol":"udp"}]},"init":{}}}"#;
        let mut ports = host_ports_from_config(json);
        ports.sort();
        assert_eq!(ports, vec![(53, "udp".to_string()), (3206, "tcp".to_string()), (3207, "tcp".to_string())]);
        assert!(host_ports_from_config("{}").is_empty());
    }

    #[test]
    fn host_ports_handles_string_number_and_out_of_range() {
        // published may be a JSON string or a number; 0 and anything above u16::MAX are dropped,
        // and a service with no ports contributes nothing.
        let json = r#"{"services":{
            "a":{"ports":[{"published":"80","target":80}]},
            "b":{"ports":[{"published":8443,"target":8443}]},
            "c":{"ports":[{"published":0,"target":5000},{"published":"0","target":5001}]},
            "d":{"ports":[{"published":70000,"target":9}]},
            "e":{"ports":[{"published":"notaport","target":9}]},
            "init":{}
        }}"#;
        let mut ports: Vec<u16> = host_ports_from_config(json).into_iter().map(|(p, _)| p).collect();
        ports.sort();
        assert_eq!(ports, vec![80, 8443]);
    }

    #[test]
    fn host_ports_tolerates_garbage_and_missing_services() {
        assert!(host_ports_from_config("not json").is_empty());
        assert!(host_ports_from_config("{}").is_empty());
        assert!(host_ports_from_config(r#"{"services":{}}"#).is_empty());
    }

    #[test]
    fn serving_services_are_those_that_publish_a_port() {
        let json = r#"{"services":{
            "web":{"ports":[{"published":"80","target":80}]},
            "db":{"ports":[{"published":5432,"target":5432}]},
            "init":{},
            "worker":{"ports":[]}
        }}"#;
        let mut names = serving_services_from_config(json);
        names.sort();
        // init (no `ports`) and worker (empty `ports`) are one-shot/background, not serving.
        assert_eq!(names, vec!["db", "web"]);
        assert!(serving_services_from_config("{}").is_empty());
        assert!(serving_services_from_config("garbage").is_empty());
    }

    #[test]
    fn without_its_compose_file_a_lab_is_reached_by_project() {
        use super::{PORTS_FILE, args};
        let dir = std::env::temp_dir().join(format!("cyberctf-compose-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PORTS_FILE), "services: {}\n").unwrap();
        assert_eq!(args(&dir, "cyberctf-x", &["down"]), ["compose", "-p", "cyberctf-x", "down"]);
        let file = crate::runtime::lab::compose_file(&dir);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "services: {}\n").unwrap();
        let a = args(&dir, "cyberctf-x", &["ps"]);
        assert_eq!(a[3..], ["-f".to_string(), file.display().to_string(), "-f".into(), PORTS_FILE.into(), "ps".into()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn removes_exactly_the_listed_containers() {
        use super::rm_args;
        assert_eq!(rm_args(""), None);
        assert_eq!(rm_args(" \n"), None);
        assert_eq!(rm_args("abc\ndef\n"), Some(vec!["rm", "-f", "abc", "def"]));
    }

    #[test]
    fn project_name_is_namespaced_per_lab() {
        assert_eq!(project("sqli"), "cyberctf-sqli");
        assert_ne!(project("a"), project("b"));
    }

    #[test]
    fn parses_both_compose_output_formats() {
        let lines = "{\"Service\":\"web\",\"State\":\"running\"}\n{\"Service\":\"db\",\"State\":\"exited\"}\n";
        let array = "[{\"Service\":\"web\",\"State\":\"running\"}]";
        assert_eq!(parse_ps(lines).len(), 2);
        assert_eq!(parse_ps(array)[0].service, "web");
        assert!(parse_ps("").is_empty());
    }

    #[test]
    fn compose_run_containers_are_one_off() {
        let out = "{\"Service\":\"isoloom-check\",\"State\":\"running\",\"Labels\":\"com.docker.compose.project=p,com.docker.compose.oneoff=True\"}\n{\"Service\":\"web\",\"State\":\"running\",\"Labels\":\"com.docker.compose.oneoff=False\"}\n{\"Service\":\"db\",\"State\":\"running\"}\n";
        let flags: Vec<(String, bool)> = parse_ps(out).iter().map(|e| (e.service.clone(), e.one_off())).collect();
        assert_eq!(flags, vec![("isoloom-check".into(), true), ("web".into(), false), ("db".into(), false)]);
    }

    #[test]
    fn parse_ps_ndjson_skips_blank_and_malformed_lines() {
        // NDJSON with surrounding whitespace, a blank line, and one unparseable line.
        let out = "  \n{\"Service\":\"web\",\"State\":\"running\",\"Health\":\"healthy\"}\n\n{bogus}\n{\"Service\":\"db\",\"State\":\"exited\"}\n";
        let entries = parse_ps(out);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].service, "web");
        assert_eq!(entries[0].health, "healthy");
        assert_eq!(entries[1].state, "exited");
    }

    #[test]
    fn parse_ps_array_with_leading_whitespace() {
        let out = "\n   [{\"Service\":\"web\",\"State\":\"running\"}]  ";
        assert_eq!(parse_ps(out).len(), 1);
        // A broken array yields nothing rather than panicking.
        assert!(parse_ps("[not valid").is_empty());
    }

    #[test]
    fn parse_ps_reads_publishers_and_defaults_missing_fields() {
        let out = r#"[{"Service":"web","State":"running","Publishers":[{"PublishedPort":8080,"TargetPort":80,"Protocol":"tcp"}]}]"#;
        let e = parse_ps(out);
        assert_eq!(e[0].publishers.len(), 1);
        assert_eq!(e[0].publishers[0].published_port, 8080);
        assert_eq!(e[0].publishers[0].target_port, 80);
        // Image/Name/Health default to empty when absent.
        assert_eq!(e[0].image, "");
        assert_eq!(e[0].name, "");
    }

    #[test]
    fn a_port_without_a_target_leaves_every_pin_out() {
        // `docker compose config` always writes `target`; should it not, no override is written
        // at all (every service then keeps ephemeral ports), rather than a half override.
        let json = r#"{"services":{"a":{"ports":[{"published":""}]},"b":{"ports":[{"target":80}]}}}"#;
        assert_eq!(pinned_ports(json, |_, _| Some(40000)), None);
        // A target past u16, likewise.
        assert_eq!(pinned_ports(r#"{"services":{"a":{"ports":[{"target":70000}]}}}"#, |_, _| Some(40000)), None);
    }

    #[test]
    fn pins_treat_unreadable_published_values_as_ephemeral_and_keep_odd_host_ips() {
        // `published: true` or an object isn't a port: treated as ephemeral (pinned). An IPv6
        // host address is bracketed; an empty one is dropped; a protocol is kept verbatim.
        let json =
            r#"{"services":{"a":{"ports":[{"target":80,"published":true,"host_ip":"::1","protocol":"udp"},{"target":443,"published":"8443","host_ip":""}]}}}"#;
        let yaml = pinned_ports(json, |_, _| Some(40000)).unwrap();
        assert!(yaml.contains("- \"[::1]:40000:80/udp\"\n"), "{yaml}");
        assert!(yaml.contains("- \"8443:443/tcp\"\n"), "{yaml}");
    }

    #[test]
    fn config_readers_take_string_targets_and_drop_zero_or_missing_hosts() {
        let json = r#"{"services":{"web":{"ports":[{"target":"80","published":"8080"},{"target":81,"published":0},{"target":82},{"target":83,"published":"65535"},{"target":84,"published":65536}]}}}"#;
        assert_eq!(published_from_config(json), vec![("web".to_string(), 80, "tcp".to_string(), 8080), ("web".to_string(), 83, "tcp".to_string(), 65535)]);
        let mut hosts = host_ports_from_config(json);
        hosts.sort();
        assert_eq!(hosts, [(8080, "tcp".to_string()), (65535, "tcp".to_string())]);
        // `ports` that isn't a list, or `services` that isn't a map: nothing, no panic.
        assert!(published_from_config(r#"{"services":{"web":{"ports":"8080:80"}}}"#).is_empty());
        assert!(serving_services_from_config(r#"{"services":[]}"#).is_empty());
        assert!(serving_services_from_config(r#"{"services":{"web":{"ports":"8080:80"}}}"#).is_empty());
    }

    #[test]
    fn project_names_keep_the_id_verbatim() {
        // Ids are validated before they get here (64 chars, [A-Za-z0-9_-]); the longest is kept whole.
        let long = "a".repeat(64);
        assert_eq!(project(&long), format!("cyberctf-{long}"));
        assert_eq!(project("Up_Per"), "cyberctf-Up_Per");
    }
}

#[cfg(test)]
mod proptests;
