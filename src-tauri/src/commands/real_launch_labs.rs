//! One local lab through the launcher's real launch path, end to end, at the adapter layer. It
//! imports and starts the lab as `lab_launch_local` does (its worker body, `labs::run_local`),
//! reads it through the `lab_status` and `lab_check` commands, parks and resumes it through the
//! runtime operations, then stops it and asserts nothing of it is left. This covers the import and
//! start-locked path a real launch takes, which the engine-level `real_corpus_labs` does not. The
//! assertion helpers are shared with the other real-lab tests (`runtime::real_labs`).
//!
//! Opt-in (it needs Docker and a local lab folder whose image is cached or buildable in time):
//!
//! ```text
//! CYBERCTF_TEST_LOCAL_LAB=/path/to/lab \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 real_launch_labs
//! ```

use crate::labs::{local_id, run_local};
use crate::runtime::real_labs::{check, leftovers, log};
use crate::runtime::{LabStatus, Park, Runtime, lab_check, lab_status, park_lab, resume_lab, stop_lab};

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
    let id = local_id(&src).expect("a lab folder with an isoloom.yml");
    let mut f = Vec::new();

    let started = run_local(&app, &id, &src, |l: String| println!("[{id}] {l}")).await;
    check(started.is_ok(), format!("{id}: it imports and starts ({:?})", started.as_ref().err()), &mut f);
    if started.is_ok() {
        let s = lab_status(app.clone(), id.clone(), Runtime::Docker).await;
        check(s.as_ref().is_ok_and(up), format!("{id}: lab_status reports it up ({})", s.as_ref().err().map(ToString::to_string).unwrap_or_default()), &mut f);
        let c = lab_check(app.clone(), id.clone(), Runtime::Docker).await;
        check(c.as_ref().is_ok_and(|c| c.available && c.ok && !c.results.is_empty()), format!("{id}: lab_check passes ({c:?})"), &mut f);
        let parked = park_lab(&app, &id, Runtime::Docker, Park::Shutdown, log(&id)).await;
        check(parked.is_ok(), format!("{id}: it parks ({:?})", parked.err()), &mut f);
        let resumed = resume_lab(&app, &id, Runtime::Docker, log(&id)).await;
        check(resumed.is_ok(), format!("{id}: it resumes ({:?})", resumed.err()), &mut f);
        let again = lab_status(app.clone(), id.clone(), Runtime::Docker).await;
        check(
            again.as_ref().is_ok_and(up),
            format!("{id}: it is up again after resume ({})", again.as_ref().err().map(ToString::to_string).unwrap_or_default()),
            &mut f,
        );
    }

    let stopped = stop_lab(&app, &id, Runtime::Docker, log(&id)).await;
    check(stopped.is_ok(), format!("{id}: it stops ({:?})", stopped.err()), &mut f);
    let left = leftovers(&id).await;
    check(left.is_empty(), format!("{id}: nothing of it is left ({left:?})"), &mut f);

    assert!(f.is_empty(), "the lab failed through the command path:\n{}", f.join("\n"));
}
