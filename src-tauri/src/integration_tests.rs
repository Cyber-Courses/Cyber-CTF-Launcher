//! Real labs, end to end, on the real tools: Docker, local hypervisors, cloud accounts and
//! servers, driven through the launcher's own code. Every test here is opt-in (`#[ignore]`): it
//! reads its knobs from `CYBERCTF_TEST_*` and returns early without them. `scripts/integration-tests.sh`
//! runs them by tier; docs/testing/integration.md says what each one guards against.

mod attack_box;
mod commands;
mod corpus;
mod docker;
mod docker_vm;
mod flag_contract;
mod per_machine;
mod replace;
mod servers;
pub(crate) mod support;
mod vm;

/// A scenario starts and stops its lab through the launcher's own functions (`launch::start_*`,
/// `runtime::start`, `lifecycle::stop_run`, `stop_lab`), never by calling the runner's `up` or
/// `down` on a run it opened itself: what the launcher does around the runner (generating the
/// files, the form's marks, opening UTM and trying its passing `-600` refusal again) is what a
/// real run goes through. `docker_vm`'s scenario once built its start by hand, so it skipped that
/// retry and failed on a UTM run where the app would not have.
#[cfg(test)]
mod start_path {
    use std::path::Path;

    /// The scenarios that drive a run they open themselves, each with why: the server ones take
    /// the host's connection from their knobs, with no saved host for the launcher's start to read.
    const OPENS_ITS_OWN_RUN: &[(&str, &str)] = &[("servers.rs", "a server's connection from its knobs, not a saved host")];

    /// The runs a scenario opens itself: `LabRun::…`, `ServerRun::…`, `CloudRun::…`.
    const RUNS: [&str; 3] = ["LabRun::", "ServerRun::", "CloudRun::"];

    /// What starts or stops a run.
    const CHANGES: [&str; 2] = [".up(", ".down("];

    /// Where `code` starts or stops a run it opened itself: a statement that opens one and calls
    /// `up` or `down` on it, or `up` or `down` on a variable bound to one. Reading it (`status`,
    /// `check`) and handing it to the launcher (`lifecycle::stop_run(LabRun::…)`) are fine.
    fn direct_changes(code: &str) -> Vec<String> {
        let statements: Vec<&str> = code.split(';').collect();
        let opened = |s: &str| RUNS.iter().any(|r| s.contains(r));
        let changes = |s: &str, on: &str| CHANGES.iter().any(|c| s.contains(&format!("{on}{c}")));
        let bound: Vec<String> =
            statements.iter().filter(|s| opened(s)).filter_map(|s| s.trim().strip_prefix("let ")?.split([' ', ':', '=']).next().map(str::to_owned)).collect();
        statements
            .iter()
            .filter(|s| (opened(s) && changes(s, "") && !s.contains("stop_run(")) || bound.iter().any(|v| changes(s, v)))
            .map(|s| s.trim().to_owned())
            .collect()
    }

    #[test]
    fn a_scenario_starts_and_stops_its_lab_the_way_the_launcher_does() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/integration_tests");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().extension().is_none_or(|x| x != "rs") || OPENS_ITS_OWN_RUN.iter().any(|(f, _)| *f == name) {
                continue;
            }
            let code = std::fs::read_to_string(entry.path()).unwrap();
            found.extend(direct_changes(&code).into_iter().map(|s| format!("{name}: {s}")));
        }
        assert!(found.is_empty(), "start or stop the lab through launch:: / lifecycle::, not the runner:\n{}", found.join("\n"));
    }

    #[test]
    fn a_run_started_by_hand_is_told_from_one_read_or_handed_over() {
        // docker_vm.rs as it was: the run opened and brought up in one expression.
        let by_hand = "let up = async {\n lab::prepare(&dir, t)?;\n isothrum::LabRun::docker_in_vm(&home, &dir, &id)?.up(&[], Some(p), &mut log(&id)).await\n}";
        assert_eq!(direct_changes(by_hand).len(), 1);
        // A run bound to a variable, then brought up or down.
        assert_eq!(direct_changes("let run = ServerRun::new(&home, &dir, id).unwrap();\nlet up = run.up(&place, &[], &mut log).await;\nlet down = run.down(&place, &mut log).await;").len(), 2);
        // Read, checked, or handed to the launcher's stop: fine.
        for fine in [
            "isothrum::LabRun::docker_in_vm(&home, &dir, &id).unwrap().status().await;",
            "isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.check().await;",
            "let stopped = lifecycle::stop_run(isothrum::LabRun::docker_in_vm(&home, &dir, &id).unwrap(), &dir, log(&id)).await;",
            "let started = launch::start_docker_in_vm_run(&home, &dir, &id, launch::VmStart { provider, env: &[] }, &mut log(&id)).await;",
        ] {
            assert!(direct_changes(fine).is_empty(), "{fine}");
        }
        // Every exemption names its reason.
        assert!(OPENS_ITS_OWN_RUN.iter().all(|(_, why)| !why.is_empty()));
    }
}
