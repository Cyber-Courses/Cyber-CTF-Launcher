//! Talking to the `docker compose` CLI for one lab, and parsing what it prints. Every
//! compose call in the engine goes through here, so the project/file flags live in one place.

use std::path::Path;

use serde::Deserialize;

use crate::error::Result;
use crate::platform::exec::run_read;

// One compose project per lab, so labs never collide and can be cleaned up by name.
pub fn project(id: &str) -> String {
    format!("cyberctf-{id}")
}

/// Upgrade only: the suffixes of the attack box containers older versions of the launcher ran
/// beside a lab's project (`<project>-isoloom-access`, `-attacker`, `-exegol`). Those were bare
/// `docker run`s with no Compose project, so a teardown by the project label never removes them.
/// Today's box is a service of the lab's own project; nothing makes these any more.
pub const LEGACY_ATTACK_BOX_SUFFIXES: [&str; 3] = ["-isoloom-access", "-attacker", "-exegol"];

/// Upgrade only: the names a box an older version left beside `project` can carry
/// ([`LEGACY_ATTACK_BOX_SUFFIXES`]), for a teardown to remove it by name.
pub fn legacy_attack_box_names(project: &str) -> [String; 3] {
    LEGACY_ATTACK_BOX_SUFFIXES.map(|s| format!("{project}{s}"))
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
/// `--context <name>` when the lab in `dir` was pinned to a Docker engine at launch, else nothing:
/// the launcher's own docker/compose calls for the lab then go to that engine (the runner pins its
/// calls through `EngineProcesses`). The flag takes precedence over the current context.
pub(super) fn context_prefix(dir: &Path) -> Vec<String> {
    match crate::runtime::paths::engine(dir) {
        Some(ctx) => vec!["--context".to_string(), ctx],
        None => Vec::new(),
    }
}

fn args(dir: &Path, project: &str, rest: &[&str]) -> Vec<String> {
    let file = crate::runtime::lab::compose_file(dir);
    let mut a = context_prefix(dir);
    a.extend(["compose".to_string(), "-p".into(), project.into()]);
    if file.is_file() {
        a.extend(["-f".into(), file.display().to_string()]);
        if dir.join(PORTS_FILE).is_file() {
            a.extend(["-f".into(), PORTS_FILE.into()]);
        }
    }
    a.extend(rest.iter().map(std::string::ToString::to_string));
    a
}

/// Runs a compose subcommand and returns its stdout.
pub(super) async fn output(dir: &Path, project: &str, rest: &[&str]) -> Result<String> {
    let a = args(dir, project, rest);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    run_read("docker", &a, Some(dir)).await
}

#[derive(Deserialize)]
pub(super) struct PsEntry {
    #[serde(rename = "Service")]
    pub(super) service: String,
    #[serde(rename = "State")]
    pub(super) state: String,
    #[serde(rename = "Image", default)]
    pub(super) image: String,
    #[serde(rename = "Name", default)]
    pub(super) name: String,
    /// `null` from some Compose versions for a container without ports: as none.
    #[serde(rename = "Publishers", default, deserialize_with = "null_as_empty")]
    pub(super) publishers: Vec<Publisher>,
}

/// A JSON `null` where a list is expected reads as an empty list.
fn null_as_empty<'de, D: serde::Deserializer<'de>, T: serde::Deserialize<'de>>(d: D) -> std::result::Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
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

#[cfg(test)]
mod tests {
    use super::{legacy_attack_box_names, parse_ps, project};

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
    fn a_lab_pinned_to_an_engine_runs_its_compose_on_that_context() {
        use super::{args, context_prefix};
        use crate::runtime::paths::ENGINE_MARKER;
        let dir = std::env::temp_dir().join(format!("cyberctf-compose-ctx-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        // No engine recorded: no `--context`, today's behaviour (the current context).
        assert!(context_prefix(&dir).is_empty());
        assert_eq!(args(&dir, "cyberctf-x", &["down"]), ["compose", "-p", "cyberctf-x", "down"]);
        // Pinned to an engine: every compose call carries `--context <ctx>` first, whatever the
        // machine's current context is now (so stop reaches the engine it was started on).
        std::fs::write(dir.join(ENGINE_MARKER), "orbstack\n").unwrap();
        assert_eq!(context_prefix(&dir), ["--context", "orbstack"]);
        assert_eq!(args(&dir, "cyberctf-x", &["down"]), ["--context", "orbstack", "compose", "-p", "cyberctf-x", "down"]);
        std::fs::remove_dir_all(dir).unwrap();
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
    fn parse_ps_ndjson_skips_blank_and_malformed_lines() {
        // NDJSON with surrounding whitespace, a blank line, and one unparseable line.
        let out = "  \n{\"Service\":\"web\",\"State\":\"running\",\"Health\":\"healthy\"}\n\n{bogus}\n{\"Service\":\"db\",\"State\":\"exited\"}\n";
        let entries = parse_ps(out);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].service, "web");
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

    #[test]
    fn legacy_attack_box_names_cover_every_older_name() {
        // Tearing a lab down removes these by name: an older version's box is a bare container
        // with no project label, so a project-label removal never catches it.
        assert_eq!(
            legacy_attack_box_names("cyberctf-sqli"),
            ["cyberctf-sqli-isoloom-access", "cyberctf-sqli-attacker", "cyberctf-sqli-exegol"].map(String::from)
        );
    }
}
