//! A whole setup run on this machine, step by step ([`walk`], isothrum-machine's): each step of
//! the plan run by the function the setup window's command for it calls, the machine checked
//! again after each. What the window does one click at a time, for a caller without one: the
//! command line's `machine setup` and the test rig's headless setup (`integration_tests::setup`)
//! both walk it.

use serde::Serialize;

use super::{Action, Wanted, WindowsFeature, distribution_said};
use crate::machine::system::{self, SystemReport, docker};
use crate::machine::tools::install::{self, name};
use crate::platform::AppDirs;
use isothrum_machine::setup::Steps;
use isothrum_machine::setup::{LeftAlone, Shared};
pub use isothrum_machine::setup::{OnFailure, Walked};

/// How a step the setup runs (or leaves alone) ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "note", rename_all = "camelCase")]
pub enum Outcome {
    /// It did what it is for (`"Docker installed"`).
    Done(String),
    /// The player's to take, or nothing to do: the note says why.
    Skipped(String),
    /// It failed, or can't run here: the note says why.
    Failed(String),
}

/// What a walk tells as it goes.
pub type Event = isothrum_machine::setup::Event<Outcome>;

/// A step the setup doesn't run itself: the player's to take (skipped) or one that can't run here
/// (failed). None for a step the setup runs.
pub fn left_alone(action: &Action) -> Option<Outcome> {
    Some(match isothrum_machine::setup::left_alone(action)? {
        LeftAlone::Distribution { dependency } => Outcome::Skipped(distribution_said(dependency)),
        LeftAlone::LinksOut { url } => Outcome::Skipped(format!("links out: {}", url.as_deref().unwrap_or("its vendor's site"))),
        LeftAlone::CantRun { reason } => Outcome::Failed(format!("can't run here: {}", crate::runtime::providers::said(&reason))),
        other => Outcome::Failed(format!("{other:?}")),
    })
}

/// What the setup would do for a step it runs, said by a dry run (`would …`); None for one it
/// leaves alone ([`left_alone`]).
pub fn would(action: &Action) -> Option<String> {
    Some(match action {
        Action::Install { dependency } => format!("would install {}", name(*dependency)),
        Action::Plugin { plugin } => format!("would install the Vagrant plugin {plugin}"),
        Action::StartEngine { engine } => format!("would start {}", engine.id()),
        Action::UseEngine { engine } => format!("would point Docker at {}", engine.id()),
        Action::ShareVirtualization => "would let VirtualBox share the CPU's virtualization with KVM".to_owned(),
        Action::Restart { feature } => format!(
            "would restart the computer to turn {} on",
            match feature {
                WindowsFeature::Wsl => "WSL 2",
                WindowsFeature::Hyperv => "Hyper-V",
            }
        ),
        Action::Relogin => "would sign out and back in".to_owned(),
        Action::Blocked { reason } if action.ends_pass() => format!("would restart the computer: {}", crate::runtime::providers::said(reason)),
        Action::Manual { .. } | Action::Blocked { .. } => return None,
    })
}

/// Runs `action` by the function the window's command for it calls, its output into `on_line`.
/// A restart or a new login is never run: it ends the pass, the caller's to say.
pub async fn run_step(dirs: &AppDirs, action: &Action, mut on_line: impl FnMut(String)) -> Outcome {
    if let Some(left) = left_alone(action) {
        return left;
    }
    let done = match action {
        Action::Install { dependency } => {
            install::install_dependency(dirs.clone(), *dependency, &mut on_line).await.map(|()| format!("{} installed", name(*dependency)))
        }
        Action::Plugin { plugin } => install::install_vagrant_plugin(plugin.clone(), &mut on_line).await.map(|()| format!("{plugin} installed")),
        Action::StartEngine { engine } => docker::docker_start_engine(engine.id().to_owned()).await.map(|()| format!("{} started", engine.id())),
        Action::UseEngine { engine } => docker::docker_use_engine(engine.id().to_owned()).await.map(|()| format!("Docker uses {}", engine.id())),
        Action::ShareVirtualization => install::share_virtualization(&mut on_line).await.map(|shared| match shared {
            Shared::Now => "KVM shares the CPU's virtualization with VirtualBox".to_owned(),
            Shared::AfterRestart => "KVM shares the CPU's virtualization with VirtualBox from the next restart".to_owned(),
        }),
        Action::Restart { .. } | Action::Relogin | Action::Manual { .. } | Action::Blocked { .. } => {
            return Outcome::Skipped(would(action).unwrap_or_default().replacen("would ", "needs to ", 1));
        }
    };
    done.map_or_else(|e| Outcome::Failed(e.to_string()), Outcome::Done)
}

/// Walks the setup `wanted` from the machine `report` describes: each step run by [`run_step`]
/// and told to `on`, the machine checked again after it, until none is left, or (`on_failure`)
/// one fails.
pub async fn walk(dirs: &AppDirs, wanted: &Wanted, report: SystemReport, on_failure: OnFailure, on: impl FnMut(Event)) -> Walked {
    let first = (super::machine(&report), report.setup);
    isothrum_machine::setup::walk(
        wanted,
        first,
        on_failure,
        Steps {
            run: async |step: &isothrum_machine::setup::Step, line: &mut dyn FnMut(String)| run_step(dirs, &step.action, line).await,
            failed: |outcome: &Outcome| matches!(outcome, Outcome::Failed(_)),
            check: async || {
                let report = system::system_check(dirs.clone()).await;
                (super::machine(&report), report.setup)
            },
        },
        on,
    )
    .await
}
