//! One local lab through the launcher's real launch path, end to end, at the adapter layer. It
//! imports and starts the lab as `lab_launch_local` does (its worker body, `labs::run_local`),
//! reads it through the `lab_status` and `lab_check` commands, parks and resumes it through the
//! runtime operations, then stops it and asserts nothing of it is left. This covers the import and
//! start-locked path a real launch takes, which the engine-level `corpus` tests do not. The
//! assertion helpers are shared with the other integration tests (`integration_tests::support`).
//!
//! Opt-in (it needs Docker and a local lab folder whose image is cached or buildable in time):
//!
//! ```text
//! CYBERCTF_TEST_LOCAL_LAB=/path/to/lab \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::commands::a_local_lab_launches_and_tears_down_through_the_commands
//! ```

use super::support::{check, leftovers, log, scenario};
use crate::commands::labs::{lab_check, lab_status};
use crate::labs::{local_id, run_local};
use crate::runtime::{LabStatus, Park, Runtime, park_lab, resume_lab, stop_lab};

/// Every non-infra machine of a running lab is up.
fn up(s: &LabStatus) -> bool {
    s.running && !s.machines.is_empty() && s.machines.iter().filter(|m| !m.infra).all(|m| m.state == "running")
}

#[tokio::test]
#[ignore = "needs Docker and a local lab folder (CYBERCTF_TEST_LOCAL_LAB)"]
async fn a_local_lab_launches_and_tears_down_through_the_commands() {
    let Some(src) = std::env::var_os("CYBERCTF_TEST_LOCAL_LAB") else { return };
    let src = std::path::PathBuf::from(src);
    let t = crate::testing::test_app::TestApp::new();
    let app = t.handle().clone();
    let dirs = crate::platform::AppDirs::from(&app);
    let id = local_id(&src).expect("a lab folder with an isoloom.yml");
    let mut f = Vec::new();

    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let started = run_local(&dirs, &id, &src, |l: String| println!("[{id}] {l}")).await;
            check(started.is_ok(), format!("{id}: it imports and starts ({:?})", started.as_ref().err()), f);
            if started.is_err() {
                return;
            }
            let s = lab_status(app.clone(), id.clone(), Runtime::Docker).await;
            check(s.as_ref().is_ok_and(up), format!("{id}: lab_status reports it up ({})", s.as_ref().err().map(ToString::to_string).unwrap_or_default()), f);
            let c = lab_check(app.clone(), id.clone(), Runtime::Docker).await;
            check(c.as_ref().is_ok_and(|c| c.available && c.ok && !c.results.is_empty()), format!("{id}: lab_check passes ({c:?})"), f);
            let parked = park_lab(&dirs, &id, Runtime::Docker, Park::Shutdown, log(&id)).await;
            check(parked.is_ok(), format!("{id}: it parks ({:?})", parked.err()), f);
            let resumed = resume_lab(&dirs, &id, Runtime::Docker, log(&id)).await;
            check(resumed.is_ok(), format!("{id}: it resumes ({:?})", resumed.err()), f);
            let again = lab_status(app.clone(), id.clone(), Runtime::Docker).await;
            check(
                again.as_ref().is_ok_and(up),
                format!("{id}: it is up again after resume ({})", again.as_ref().err().map(ToString::to_string).unwrap_or_default()),
                f,
            );
        },
        async |f: &mut Vec<String>| {
            let stopped = stop_lab(&dirs, &id, Runtime::Docker, std::future::ready(()), log(&id)).await;
            check(stopped.is_ok(), format!("{id}: it stops ({:?})", stopped.err()), f);
            let left = leftovers(&id).await;
            check(left.is_empty(), format!("{id}: nothing of it is left ({left:?})"), f);
        },
    )
    .await;

    assert!(f.is_empty(), "the lab failed through the command path:\n{}", f.join("\n"));
}
