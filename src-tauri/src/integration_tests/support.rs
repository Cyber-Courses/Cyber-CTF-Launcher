//! What the real-lab tests share: a scenario's run (its checks, then its teardown whatever happens
//! in them: [`scenario`]), its knobs, its checks (each failure collected, so the lab is always
//! stopped after) and its log; by concern, a lab installed for a test and run through the runner,
//! and what a lab leaves on Docker ([`lab`]), the attack box driven as the lab page drives it,
//! through the console the lab page opens ([`attack_box`]), and the JSON records an entry point
//! the test rig calls writes for it ([`results`]).

pub(crate) mod attack_box;
pub(crate) mod lab;
pub(crate) mod results;

use crate::platform::unwind::{CatchUnwind, panic_message};

/// How a test says it returned because a knob it needs isn't set: `tests/integration/run.sh`
/// records the scenario as skipped on this line, where cargo counts the early return as a pass.
pub(crate) const SKIPPED: &str = "integration test skipped, not set:";

/// How a test says it returned because the launcher refuses its lab on the provider it was given
/// (Isoloom's own rule, as the launch says it: a lab whose VMs are on a network can't run on UTM,
/// whose Vagrant provider gives a VM no private network): `tests/integration/run.sh` records the
/// scenario as skipped, with the launcher's words as the reason. The scenario was never started.
pub(crate) const REFUSED: &str = "integration test skipped, refused by the launcher:";

/// Whether the launcher refuses to start the lab installed in `dir` as `target` on `provider`,
/// before anything runs, through the launch's own check ([`crate::runtime::launch::refusal_here`]):
/// said on the test's output ([`REFUSED`], the refusal's words) so the test returning on it is
/// recorded as skipped. Any other refusal, or one the launch makes later, is the scenario's
/// failure as before. A lab that can't be read isn't refused here: its start says why.
pub(crate) fn refused_by_launcher(dir: &std::path::Path, target: crate::runtime::lab::Target, provider: crate::runtime::providers::Provider) -> bool {
    match crate::runtime::launch::refusal_here(dir, target, provider) {
        Ok(Some(why)) => {
            println!("{REFUSED} {why}");
            true
        }
        Ok(None) | Err(_) => false,
    }
}

/// Knobs renamed to `CYBERCTF_TEST_<AREA>_<WHAT>` whose old names the workspace's test rig still
/// sets: (new, old). The old name is read when the new one isn't set. Remove each pair once the rig
/// sets the new name (tests/README.md, "Knobs").
const OLD_NAMES: &[(&str, &str)] =
    &[("CYBERCTF_TEST_BOX_SHARED_FOLDER", "CYBERCTF_TEST_SHARED_FOLDER"), ("CYBERCTF_TEST_PVE_RUNNER_HOME", "CYBERCTF_TEST_RUNNER_HOME")];

/// Knob `name` from the environment (or its old name, [`OLD_NAMES`]); None when unset or blank.
pub(crate) fn var(name: &str) -> Option<String> {
    let set = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
    set(name).or_else(|| OLD_NAMES.iter().find(|(new, _)| *new == name).and_then(|(_, old)| set(old)))
}

/// Knob `name`, the test's input from the environment; None when it is unset or blank, said on
/// the test's output ([`SKIPPED`]) so the test returning on it isn't taken for a pass.
pub(crate) fn knob(name: &str) -> Option<String> {
    let value = var(name);
    if value.is_none() {
        println!("{SKIPPED} {name}");
    }
    value
}

/// What a scenario keeps its failed checks in: a list of them, or a state of its own holding one.
pub(crate) trait Checks {
    fn failures(&mut self) -> &mut Vec<String>;
}

impl Checks for Vec<String> {
    fn failures(&mut self) -> &mut Vec<String> {
        self
    }
}

/// Runs a scenario's `body` (the lab started, its checks), then its `teardown` (the lab stopped,
/// what it left checked) whatever happens in the body: a panic there is a failed check of its own,
/// and the teardown still runs. A lab a scenario leaves behind fails the ones after it (a UTM VM
/// still registered makes every later docker-vm import fail). A panic in the teardown is a failed
/// check too. Each part's failures go to `state`.
pub(crate) async fn scenario<S: Checks>(state: &mut S, body: impl AsyncFnOnce(&mut S), teardown: impl AsyncFnOnce(&mut S)) {
    if let Err(panic) = (CatchUnwind(Box::pin(body(state)))).await {
        check(false, format!("the scenario ran to its end (it panicked: {})", panic_message(&*panic)), state.failures());
    }
    if let Err(panic) = (CatchUnwind(Box::pin(teardown(state)))).await {
        check(false, format!("the teardown ran to its end (it panicked: {})", panic_message(&*panic)), state.failures());
    }
}

/// `url` fetched on this machine (15 s at most): its status, or why it couldn't be.
pub(crate) async fn fetch(url: &str) -> std::result::Result<reqwest::StatusCode, String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    client.get(url).send().await.map(|r| r.status()).map_err(|e| e.to_string())
}

/// The test's log for the lab `id`: each line printed, tagged with the id.
pub(crate) fn log(id: &str) -> impl FnMut(String) + use<> {
    let tag = id.to_string();
    move |l: String| println!("[{tag}] {l}")
}

/// One check: an `ok:` line when it holds, else a `FAILED:` line and `what` among the `failures`.
pub(crate) fn check(ok: bool, what: String, failures: &mut Vec<String>) {
    if ok {
        println!("ok: {what}");
    } else {
        println!("FAILED: {what}");
        failures.push(what);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_knob_is_none_and_said_as_the_runner_reads_it() {
        assert_eq!(knob("CYBERCTF_TEST_NEVER_SET_BY_ANYONE"), None);
        // A renamed knob is read by its old name too, while the rig sets that one.
        assert!(OLD_NAMES.iter().all(|(new, old)| new.starts_with("CYBERCTF_TEST_") && old.starts_with("CYBERCTF_TEST_") && new != old));
        // tests/integration/run.sh records a scenario as skipped on this line.
        assert!(include_str!("../../../tests/integration/run.sh").contains(&format!("KNOB_UNSET=\"{SKIPPED}\"")));
        // And as skipped, with the launcher's words, on this one.
        assert!(include_str!("../../../tests/integration/run.sh").contains(&format!("REFUSED=\"{REFUSED}\"")));
    }

    /// The lab both forms of which take a private network (as Isoloom's solo-web's VM does).
    const NETWORKED: &str = "version: 1\nname: both\nnetworks:\n  lab: { cidr: 10.70.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n    app: { image: nginx:1.27, steps: [{ run: setup.sh }] }\n";

    #[test]
    fn a_lab_the_launcher_refuses_on_a_provider_is_skipped_with_its_words_and_one_it_runs_is_not() {
        use crate::runtime::lab::Target;
        use crate::runtime::providers::Provider;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("networked", &[("isoloom.yml", NETWORKED), ("setup.sh", "true\n")]);
        // UTM: refused, as the start itself refuses it, and nothing is generated for it.
        let why = crate::runtime::launch::refusal_here(&dir, Target::Vagrant, Provider::Utm).unwrap().expect("refused on UTM");
        assert!(why.to_string().starts_with("This lab can't run on UTM yet: ") && why.to_string().contains("vagrant_utm"), "{why}");
        assert!(refused_by_launcher(&dir, Target::Vagrant, Provider::Utm));
        assert!(!dir.join(".isoloom").exists(), "the check generates nothing");
        // A hypervisor the lab runs on: not refused, so the scenario runs, and a start that
        // fails there is its failure.
        for provider in [Provider::Virtualbox, Provider::Libvirt, Provider::Qemu] {
            assert!(!refused_by_launcher(&dir, Target::Vagrant, provider), "{provider:?}");
        }
        // Not a lab at all: not a refusal of the provider; the start says why, as a failure.
        let empty = t.lab("empty", &[]);
        assert!(!refused_by_launcher(&empty, Target::Vagrant, Provider::Utm));
    }

    #[tokio::test]
    async fn a_scenario_that_panics_is_still_torn_down_and_says_where_it_stopped() {
        // Regression: a scenario's `expect` panicked before its teardown, its UTM VM stayed
        // registered, and every docker-vm scenario after it failed.
        let mut failures = Vec::new();
        let torn_down = std::cell::Cell::new(false);
        scenario(
            &mut failures,
            async |f: &mut Vec<String>| {
                check(true, "the lab starts".into(), f);
                tokio::task::yield_now().await;
                panic!("a console into the box: no such machine");
            },
            async |f: &mut Vec<String>| {
                torn_down.set(true);
                check(false, "nothing of it is left".into(), f);
            },
        )
        .await;
        assert!(torn_down.get(), "the teardown ran");
        assert_eq!(failures, ["the scenario ran to its end (it panicked: a console into the box: no such machine)", "nothing of it is left"]);
        // A body that ends, and a teardown that panics: said too.
        let mut failures = Vec::new();
        scenario(&mut failures, async |_: &mut Vec<String>| {}, async |_: &mut Vec<String>| panic!("{}", String::from("stop failed"))).await;
        assert_eq!(failures, ["the teardown ran to its end (it panicked: stop failed)"]);
    }
}
