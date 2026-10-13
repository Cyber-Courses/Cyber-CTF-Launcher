//! The lab's self-check: its Isoloom `checks:` (the lab's own scripts and declared probes, and
//! the ones Isoloom derives from `services` and `reach`), run by the Compose file's check
//! runners (profile `check`), one per position, read into a per-check table.

use std::path::Path;

use isoloom_core::checks::{self, Line};

use super::compose;
use crate::error::Result;

/// One check's outcome.
#[derive(serde::Serialize, Clone, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabCheckResult")]
pub struct CheckResult {
    pub name: String,
    /// Where it ran: "from web", or "from the environment's networks".
    pub from: String,
    pub ok: bool,
    /// Why it failed (empty when it passed).
    pub reason: String,
}

/// Result of a lab's self-verification.
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabCheck")]
pub struct Check {
    /// The lab has checks (its own, or derived from its spec), so verification is possible.
    pub available: bool,
    /// Every check passed: the lab behaves, and the challenge is still in a solvable state.
    pub ok: bool,
    /// The runners' combined output.
    pub output: String,
    /// Every check, in the order it ran.
    pub results: Vec<CheckResult>,
}

/// Isoloom's stand-in for the lab's access machine (the player's seat) and its routes, as named in
/// its Compose file: the services a lab with an access machine defines.
const STAND_INS: [&str; 2] = ["isoloom-access", "isoloom-access-routes"];

/// The stand-ins the lab's Compose file defines.
fn stand_ins(compose: &str) -> Vec<&'static str> {
    STAND_INS.into_iter().filter(|n| compose.contains(&format!("\n  {n}:\n"))).collect()
}

/// Runs every check runner of the lab (compose `check` profile): containers on the lab's
/// networks that assert what the lab declares and that the intended exploit path still
/// works, so a learner who broke their box is told to reset it instead of fighting a lab that
/// can no longer be solved.
/// How long one check runner may take.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub async fn check(dir: &Path, id: &str) -> Result<Check> {
    let spec = crate::runtime::lab::spec(dir)?;
    let plan = checks::plan(&spec);
    if plan.is_empty() {
        return Ok(Check { available: false, ok: false, output: String::new(), results: Vec::new() });
    }
    let default = checks::default_position(&spec);
    let project = compose::project(id);
    let mut lines: Vec<String> = Vec::new();
    let mut results: Vec<CheckResult> = Vec::new();
    let mut ran = true;
    // The runners run with --no-deps (`compose run` would otherwise rerun the lab's completed init
    // jobs), so the stand-in for the lab's access machine, which the access runner shares its
    // network with, is started first: it is in the `check` profile, so the lab's `up` left it down.
    let compose_text = std::fs::read_to_string(crate::runtime::lab::compose_file(dir)).unwrap_or_default();
    let stand_ins = stand_ins(&compose_text);
    if !stand_ins.is_empty() {
        let mut args = vec!["--profile", "check", "up", "-d", "--wait", "--no-deps"];
        args.extend(stand_ins.iter().copied());
        if let Err(e) = compose::stream(dir, &project, &args, &[], |l| lines.push(l)).await {
            lines.push(format!("the access machine's stand-in didn't start: {e}"));
            ran = false;
        }
    }
    for (pos, group) in checks::by_position(&spec, &plan) {
        let service =
            if pos == default { crate::runtime::lab::CHECK_SERVICE.to_string() } else { format!("{}-{}", crate::runtime::lab::CHECK_SERVICE, pos.id()) };
        let from = pos.label();
        // `exec` checks run inside the machine, from their own runner (Isoloom's exec-<machine>.sh,
        // piped in); the runner beside the machine reports the others.
        let (execs, group): (Vec<&checks::Resolved>, Vec<&checks::Resolved>) = group.into_iter().partition(|c| matches!(c.probe, checks::Probe::Exec { .. }));
        let before = results.len();
        let run_args = ["--profile", "check", "run", "--rm", "--no-deps", service.as_str()];
        let run = compose::stream(dir, &project, &run_args, &[], |l| {
            record(&mut results, &from, &l);
            lines.push(l);
        });
        // A runner that hangs (a target that never answers) must not run forever: past the limit
        // the CLI is dropped, and its container, which outlives it, removed.
        let res = match tokio::time::timeout(CHECK_TIMEOUT, run).await {
            Ok(r) => r,
            Err(_) => {
                compose::remove_one_off(&project, &service).await;
                lines.push(format!("{service}: no answer within {} minutes, stopped", CHECK_TIMEOUT.as_secs() / 60));
                Err(crate::error::Error::Invalid("the check timed out".into()))
            }
        };
        // A runner that stopped before reporting: the checks it owned, failed with that reason.
        fail_unreported(&mut results, before, &group, &from);
        if res.is_err() {
            ran = false;
        }
        if let (false, checks::Position::Machine(m)) = (execs.is_empty(), &pos) {
            let file = crate::runtime::lab::compose_file(dir).with_file_name("checks").join(isoloom_core::generate::exec_runner(m));
            let from = format!("inside {m}");
            let before = results.len();
            let res = compose::stream_with_stdin(dir, &project, &["exec", "-T", m, "sh", "-s"], &file, |l| {
                record(&mut results, &from, &l);
                lines.push(l);
            })
            .await;
            fail_unreported(&mut results, before, &execs, &from);
            if res.is_err() {
                ran = false;
            }
        }
    }
    let ok = ran && results.iter().all(|r| r.ok);
    Ok(Check { available: true, ok, output: lines.join("\n"), results })
}

/// Reads one line of a check runner's output into `results` (a pass or a fail, run `from`).
fn record(results: &mut Vec<CheckResult>, from: &str, line: &str) {
    match checks::parse_line(line) {
        Some(Line::Pass(name)) => results.push(CheckResult { name, from: from.to_string(), ok: true, reason: String::new() }),
        Some(Line::Fail(name, why)) => results.push(CheckResult { name, from: from.to_string(), ok: false, reason: why }),
        Some(Line::End(..)) | None => {}
    }
}

/// The checks of `group` a runner didn't report on (it reported `results[before..]`, in order),
/// failed: it stopped before them.
fn fail_unreported(results: &mut Vec<CheckResult>, before: usize, group: &[&checks::Resolved], from: &str) {
    let reported = results.len() - before;
    for c in group.iter().skip(reported) {
        results.push(CheckResult { name: c.name.clone(), from: from.to_string(), ok: false, reason: "the runner stopped before this check".into() });
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckResult, check, fail_unreported, record, stand_ins};

    fn lab(spec: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-check-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
        dir
    }

    #[test]
    fn runner_lines_become_results() {
        let mut results = Vec::new();
        record(&mut results, "from web", "isoloom-check: PASS login works");
        record(&mut results, "from web", "web-1  | isoloom-check: FAIL flag readable: 404");
        record(&mut results, "from web", "isoloom-check: END 1 1");
        record(&mut results, "from web", "some other output");
        assert_eq!(
            results,
            [
                CheckResult { name: "login works".into(), from: "from web".into(), ok: true, reason: String::new() },
                CheckResult { name: "flag readable".into(), from: "from web".into(), ok: false, reason: "404".into() },
            ]
        );
    }

    #[tokio::test]
    async fn a_lab_without_checks_has_nothing_to_verify() {
        let dir = lab(
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  box:\n    networks: { lab: 10 }\n    docker: { image: alpine:3, idle: true }\n",
        );
        let c = check(&dir, "t").await.unwrap();
        assert!(!c.available && !c.ok && c.results.is_empty() && c.output.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checks_a_runner_never_reported_fail() {
        let dir = lab(
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\n  - { tcp: \"web:80\" }\n",
        );
        let spec = crate::runtime::lab::spec(&dir).unwrap();
        let plan = isoloom_core::checks::plan(&spec);
        assert!(plan.len() >= 2, "derived checks: {}", plan.len());
        let group: Vec<_> = plan.iter().collect();
        // The runner reported the first check, then stopped.
        let mut results = vec![CheckResult { name: "earlier".into(), from: "x".into(), ok: true, reason: String::new() }];
        let before = results.len();
        record(&mut results, "from the networks", &format!("isoloom-check: PASS {}", group[0].name));
        fail_unreported(&mut results, before, &group, "from the networks");
        assert_eq!(results.len(), 1 + group.len());
        assert!(results[1].ok);
        assert!(results[2..].iter().all(|r| !r.ok && r.reason == "the runner stopped before this check"));
        assert_eq!(results.last().unwrap().name, group.last().unwrap().name);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_access_stand_in_is_found_by_its_service_name() {
        let compose = "services:\n  web:\n    image: x\n  isoloom-access:\n    image: alpine\n  isoloom-access-routes:\n    image: alpine\n  isoloom-check:\n    network_mode: service:isoloom-access\n";
        assert_eq!(stand_ins(compose), ["isoloom-access", "isoloom-access-routes"]);
        // Mentioned, not defined: no stand-in.
        assert!(stand_ins("services:\n  isoloom-check:\n    network_mode: service:isoloom-access\n").is_empty());
    }
}
