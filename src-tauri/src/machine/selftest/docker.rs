//! The container self-test, run by isothrum-machine (`selftest::engine`): a throwaway
//! two-container deployment through a runner of its own. Each step is said here, in the app's
//! words.

use std::path::Path;

use isothrum_machine::selftest::Progress;
use isothrum_machine::selftest::engine::{self, Failure, Found, Step, StepProgress};

use super::Reporter;
use crate::error::{Error, Result};
use crate::runtime::selftest_report::StepState;

/// The test's image.
pub(crate) const IMAGE: &str = engine::IMAGE;

/// A step's id and label, as the page shows it.
fn named(step: Step) -> (&'static str, &'static str) {
    match step {
        Step::Engine => ("engine", "Container engine answers"),
        Step::Pull => ("pull", "Download a test image"),
        Step::Start => ("start", "Start a two-container test lab"),
        Step::Network => ("network", "Containers reach each other"),
        _ => ("port", "Lab port reachable from this machine"),
    }
}

/// What a passed step found, as its detail line.
fn detail(found: Found) -> Option<String> {
    match found {
        Found::Version(v) => Some(format!("Docker {v}")),
        Found::Image { held: true } => Some(format!("{IMAGE} (ready)")),
        Found::Image { held: false } => Some(IMAGE.to_owned()),
        Found::Reached => Some("probe → web on the lab network".to_owned()),
        Found::Address(address) => Some(address),
        _ => None,
    }
}

/// Why a step failed, as the app's error: the engine's silence as the machine check says it.
fn error(failure: Failure) -> Error {
    match failure {
        Failure::Engine(why) => crate::machine::system::docker::no_answer(why),
        other => Error::Invalid(other.to_string()),
    }
}

/// Says one step's progress through `r`.
fn say(r: &Reporter, progress: StepProgress) {
    match progress {
        Progress::Running(step) => {
            let (id, label) = named(step);
            r.send(id, label, StepState::Running, None);
        }
        Progress::Passed(step, found) => {
            let (id, label) = named(step);
            r.send(id, label, StepState::Ok, detail(found));
        }
        Progress::Failed(step, why) => {
            let (id, label) = named(step);
            r.failed(id, label, &error(why).to_string());
        }
        _ => {}
    }
}

/// The test, on the engine behind `context` (the CLI's current one when none is named), its
/// deployment in `dir`. What it pulled goes to the app's runner record (Storage cleans it).
pub(super) async fn run_test(dirs: &crate::platform::AppDirs, dir: &Path, context: Option<&str>, r: &Reporter) -> Result<()> {
    let cx = crate::runtime::providers::machine_selftest();
    let ran = engine::run(&cx, dir, context, &|p| say(r, p)).await;
    super::keep(dirs, ran.pulled).await;
    let passed = ran.passed.map_err(error);
    r.cleanup("Clean up", &passed, ran.cleaned_up.map_err(Error::Invalid), None);
    passed
}

/// Pulls the test image ahead of the test, unless it is here.
pub(super) async fn prefetch(dirs: &crate::platform::AppDirs) -> Result<()> {
    let cx = crate::runtime::providers::machine_selftest();
    let (_, pulled) = engine::pull(&cx, None).await.map_err(Error::Invalid)?;
    super::keep(dirs, pulled).await;
    Ok(())
}
