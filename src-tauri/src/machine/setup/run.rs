//! A whole setup run on this machine, step by step ([`walk`]): each step of the plan run by the
//! function the setup window's command for it calls, the machine checked again after each (an
//! install changes what comes next: Docker installed asks to start it, or for a new login). What
//! the window does one click at a time, for a caller without one: the command line's `machine
//! setup` and the test rig's headless setup (`integration_tests::setup`) both walk it.

use serde::Serialize;

use super::{Action, ManualReason, Step, Wanted, WindowsFeature, plan};
use crate::machine::system::{self, SystemReport, docker};
use crate::machine::tools::install;
use crate::platform::AppDirs;

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// A step begins: its item, its kind ([`kind`]) and what it does.
    Started {
        /// The item it is about (`docker`, `engine:docker-engine`, `virtualbox`, …).
        item: String,
        /// Its kind.
        kind: &'static str,
        /// The action, as the machine check reports it.
        action: Action,
    },
    /// A line of the step's output.
    Line {
        /// The step's item.
        item: String,
        /// The line.
        line: String,
    },
    /// The step ended.
    Ended {
        /// The step's item.
        item: String,
        /// How.
        #[serde(flatten)]
        outcome: Outcome,
    },
}

/// What a walk does once a step failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnFailure {
    /// Stop there: the command line's setup, whose caller fixes it and runs it again.
    Stop,
    /// Go on with the other steps: the test rig's pass, which reports every item.
    Continue,
}

/// Where a walk ended.
pub struct Walked {
    /// The restart or the new login that ends the pass, when one does: the next pass picks up
    /// after it.
    pub ends: Option<Step>,
    /// The items of the steps run, in order.
    pub acted: Vec<String>,
    /// The first step that failed, when one did.
    pub failed: Option<Step>,
}

/// A step the setup doesn't run itself: the player's to take (skipped) or one that can't run here
/// (failed). None for a step the setup runs.
pub fn left_alone(action: &Action) -> Option<Outcome> {
    Some(match action {
        Action::Manual { reason: ManualReason::Distribution, dependency, .. } => {
            let what = dependency.map_or("it", |d| d.name());
            Outcome::Skipped(format!("this distribution isn't supported by the guided setup yet: install {what} yourself"))
        }
        Action::Manual { reason: ManualReason::Vendor, url, .. } => Outcome::Skipped(format!("links out: {}", url.as_deref().unwrap_or("its vendor's site"))),
        Action::Blocked { reason } => Outcome::Failed(format!("can't run here: {reason}")),
        _ => return None,
    })
}

/// What the setup would do for a step it runs, said by a dry run (`would …`); None for one it
/// leaves alone ([`left_alone`]).
pub fn would(action: &Action) -> Option<String> {
    Some(match action {
        Action::Install { dependency } => format!("would install {}", dependency.name()),
        Action::Plugin { plugin } => format!("would install the Vagrant plugin {plugin}"),
        Action::StartEngine { engine } => format!("would start {}", engine.id()),
        Action::UseEngine { engine } => format!("would point Docker at {}", engine.id()),
        Action::Restart { feature } => format!(
            "would restart the computer to turn {} on",
            match feature {
                WindowsFeature::Wsl => "WSL 2",
                WindowsFeature::Hyperv => "Hyper-V",
            }
        ),
        Action::Relogin => "would sign out and back in".to_owned(),
        Action::Blocked { reason } if action.ends_pass() => format!("would restart the computer: {reason}"),
        Action::Manual { .. } | Action::Blocked { .. } => return None,
    })
}

/// An action's kind, as an event and a log's name say it.
pub fn kind(action: &Action) -> &'static str {
    match action {
        Action::Install { .. } => "install",
        Action::Plugin { .. } => "plugin",
        Action::StartEngine { .. } => "start",
        Action::UseEngine { .. } => "use",
        Action::Restart { .. } => "restart",
        Action::Relogin => "relogin",
        Action::Manual { .. } => "manual",
        Action::Blocked { .. } => "blocked",
    }
}

/// Runs `action` by the function the window's command for it calls, its output into `on_line`.
/// A restart or a new login is never run: it ends the pass, the caller's to say.
pub async fn run_step(dirs: &AppDirs, action: &Action, mut on_line: impl FnMut(String)) -> Outcome {
    if let Some(left) = left_alone(action) {
        return left;
    }
    let done = match action {
        Action::Install { dependency } => {
            install::install_dependency(dirs.clone(), *dependency, &mut on_line).await.map(|()| format!("{} installed", dependency.name()))
        }
        Action::Plugin { plugin } => install::install_vagrant_plugin(plugin.clone(), &mut on_line).await.map(|()| format!("{plugin} installed")),
        Action::StartEngine { engine } => docker::docker_start_engine(engine.id().to_owned()).await.map(|()| format!("{} started", engine.id())),
        Action::UseEngine { engine } => docker::docker_use_engine(engine.id().to_owned()).await.map(|()| format!("Docker uses {}", engine.id())),
        Action::Restart { .. } | Action::Relogin | Action::Manual { .. } | Action::Blocked { .. } => {
            return Outcome::Skipped(would(action).unwrap_or_default().replacen("would ", "needs to ", 1));
        }
    };
    done.map_or_else(|e| Outcome::Failed(e.to_string()), Outcome::Done)
}

/// Walks the setup `wanted` from the machine `report` describes: the next step of the plan not
/// done yet (a restart or a new login last, never run), run and told to `on`, the machine checked
/// again after it, until none is left, or (`on_failure`) one fails.
pub async fn walk(dirs: &AppDirs, wanted: &Wanted, mut report: SystemReport, on_failure: OnFailure, mut on: impl FnMut(Event)) -> Walked {
    let mut done: Vec<Step> = Vec::new();
    let mut failed = None;
    loop {
        let p = plan(&report, &report.setup, wanted);
        let next = p.steps.iter().find(|s| !s.action.ends_pass() && !done.contains(s)).cloned();
        let Some(step) = next.filter(|_| failed.is_none() || on_failure == OnFailure::Continue) else {
            let acted = done.into_iter().map(|s| s.item).collect();
            return Walked { ends: p.steps.into_iter().find(|s| s.action.ends_pass()), acted, failed };
        };
        on(Event::Started { item: step.item.clone(), kind: kind(&step.action), action: step.action.clone() });
        let outcome = run_step(dirs, &step.action, |line| on(Event::Line { item: step.item.clone(), line })).await;
        if matches!(outcome, Outcome::Failed(_)) && failed.is_none() {
            failed = Some(step.clone());
        }
        on(Event::Ended { item: step.item.clone(), outcome });
        done.push(step);
        report = system::system_check(dirs.clone()).await;
    }
}
