//! What the real-lab tests share: a scenario's run (its checks, then its teardown whatever happens
//! in them: [`scenario`]), its knobs, its checks (each failure collected, so the lab is always
//! stopped after) and its log; by concern, a lab installed for a test and run through the runner,
//! and what a lab leaves on Docker ([`lab`]), and the attack box driven as the lab page drives it,
//! through the console the lab page opens ([`attack_box`]).

pub(crate) mod attack_box;
pub(crate) mod lab;

use crate::platform::unwind::{CatchUnwind, panic_message};

/// How a test says it returned because a knob it needs isn't set: `tests/integration/run.sh`
/// records the scenario as skipped on this line, where cargo counts the early return as a pass.
pub(crate) const SKIPPED: &str = "integration test skipped, not set:";

/// Knob `name`, the test's input from the environment; None when it is unset or blank, said on
/// the test's output ([`SKIPPED`]) so the test returning on it isn't taken for a pass.
pub(crate) fn knob(name: &str) -> Option<String> {
    let value = std::env::var(name).ok().filter(|v| !v.trim().is_empty());
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
        // tests/integration/run.sh records a scenario as skipped on this line.
        assert!(include_str!("../../../tests/integration/run.sh").contains(&format!("KNOB_UNSET=\"{SKIPPED}\"")));
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
