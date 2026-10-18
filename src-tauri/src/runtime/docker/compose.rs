//! Talking to the `docker compose` CLI for one lab, and parsing what it prints. Every
//! compose call in the engine goes through here, so the project/file flags live in one place.

use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::platform::exec::{run_read, stream as exec_stream};

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
    if let Ok(ids) = crate::platform::exec::run("docker", &["ps", "-aq", "--filter", &filter], None).await
        && let Some(args) = rm_args(&ids)
    {
        let _ = crate::platform::exec::run("docker", &args, None).await;
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

#[cfg(test)]
mod tests {
    use super::{parse_ps, project, serving_services_from_config};

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
    fn project_names_keep_the_id_verbatim() {
        // Ids are validated before they get here (64 chars, [A-Za-z0-9_-]); the longest is kept whole.
        let long = "a".repeat(64);
        assert_eq!(project(&long), format!("cyberctf-{long}"));
        assert_eq!(project("Up_Per"), "cyberctf-Up_Per");
    }
}
