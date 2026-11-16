//! Running install and removal steps: one program each, its output streamed to the setup log.
//! Linux's are isothrum-machine's ([`isothrum_machine::install`]), their failures said here in the
//! player's words; macOS's and Windows' run here until they move too.

#![allow(clippy::disallowed_methods, reason = "the install and removal steps' runner: each step streams to the setup log through its own process")]

use std::process::Stdio;

use tokio::process::Command;

use super::exit_codes::{self, Verb};
use crate::error::{Error, Result};

/// One command of an install or removal.
pub struct Step {
    /// The program to run.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// A note shown before the step (e.g. when we open a download instead of installing).
    pub note: Option<String>,
}

/// A step that runs `program` with `args`, with no note.
pub fn step(program: impl Into<String>, args: &[&str]) -> Step {
    Step { program: program.into(), args: args.iter().map(ToString::to_string).collect(), note: None }
}

/// Without pkexec there is no graphical password prompt (Debian ships it apart from polkit).
pub const NO_PKEXEC: &str =
    "pkexec isn't installed, so Cyber CTF can't ask for your password. Install it once in a terminal (sudo apt install pkexec), then try again.";

/// An unattended root step sudo refused to run without a password: nothing was changed.
pub const SUDO_REFUSED: &str = "sudo needs a password to act as an administrator here, and none can be asked for unattended, so nothing was changed. Run it as root, or on a terminal without --yes.";

/// A Homebrew that can't update because a lock file is owned by another user (usually left by a
/// `sudo brew`), or a stale lock it thinks another update holds.
const BREW_LOCKED: &str = "Homebrew can't update because a lock file is owned by another user (this happens after running brew with sudo). Open the Terminal app and run: sudo chown -R $(whoami) \"$(brew --prefix)/var/homebrew\". Then try the install again here.";
/// A Homebrew too old to know this macOS ("unknown or unsupported macOS version: :dunno").
const BREW_OUTDATED: &str =
    "Homebrew is out of date for this version of macOS, so it can't install this. Open the Terminal app and run: brew update. Then try the install again here.";
/// A source build Homebrew refuses because the installed Xcode is older than it needs.
const BREW_XCODE_OUTDATED: &str = "Homebrew needs a newer Xcode than the one installed to build this. Update Xcode from the App Store (or delete /Applications/Xcode.app to use the lighter Command Line Tools). Then try the install again here.";

/// An install's or a removal's steps: the launcher's own (macOS, Windows), or isothrum-machine's
/// (Linux).
pub enum Plan {
    /// The launcher's.
    Own(Vec<Step>),
    /// isothrum-machine's.
    Machine(Vec<isothrum_machine::install::Step>),
}

impl Plan {
    /// How many steps it has.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        match self {
            Plan::Own(steps) => steps.len(),
            Plan::Machine(steps) => steps.len(),
        }
    }

    /// Whether it has no step.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Each step as `(program, args)`.
    #[cfg(test)]
    pub fn commands(&self) -> Vec<(String, Vec<String>)> {
        match self {
            Plan::Own(steps) => steps.iter().map(|s| (s.program.clone(), s.args.clone())).collect(),
            Plan::Machine(steps) => steps.iter().map(|s| (s.program.clone(), s.args.clone())).collect(),
        }
    }
}

/// Runs `plan`'s steps in order, each announced as `$ program args` unless it has its own note.
pub async fn run_plan(plan: &Plan, on_line: &mut impl FnMut(String)) -> Result<()> {
    match plan {
        Plan::Own(steps) => run_steps(steps, on_line).await,
        Plan::Machine(steps) => {
            for step in steps {
                on_line(match step.note {
                    Some(note) => said(note),
                    None => crate::platform::redact::redact(&isothrum_machine::install::command_line(step)).into_owned(),
                });
                run_machine_step(step, on_line).await?;
            }
            Ok(())
        }
    }
}

/// Runs one of isothrum-machine's steps through the app's runner, the tools' kept answers
/// forgotten before and after it (see [`run_step`]), its failure said in the player's words.
async fn run_machine_step(step: &isothrum_machine::install::Step, on_line: &mut impl FnMut(String)) -> Result<()> {
    crate::platform::remembered::forget_all();
    let cx = crate::runtime::providers::machine_check();
    // Its lines come through a channel: the step's future stays `Send` whatever `on_line` is.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let run = async move {
        let mut send = |line: &str| {
            // The receiver lives until the step ends: a line is never sent to no one.
            tx.send(line.to_owned()).ok();
        };
        isothrum_machine::install::run_step(&cx, step, &mut send).await
    };
    let shown = async {
        while let Some(line) = rx.recv().await {
            on_line(line);
        }
    };
    let (ran, ()) = tokio::join!(run, shown);
    crate::platform::remembered::forget_all();
    ran.map_err(step_failed)
}

/// What one of isothrum-machine's steps is, in the player's words, said before it instead of its
/// command line.
fn said(note: isothrum_machine::install::Note) -> String {
    use isothrum_machine::install::{Dependency as Machine, Note};
    let name = |dep| super::install::Dependency::of_machine(dep).name_on(crate::platform::host::Os::Macos);
    match note {
        Note::Downloading(dep) => format!("Downloading {}…", name(dep)),
        Note::QemuUpdatingHomebrewFirst => "Installing QEMU (updating Homebrew first if it is out of date for this macOS).".into(),
        Note::TrustingAzureTap => "Trusting the azure/azure-cli tap (Homebrew 7 asks for it; older versions don't).".into(),
        Note::AzureCliCask => "Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into(),
        Note::DownloadingVmwareUtility { version } => format!("Downloading HashiCorp's Vagrant VMware Utility {version}…"),
        Note::Removing(Machine::Docker) => "Running Docker Desktop's own uninstaller…".into(),
        Note::Removing(Machine::Virtualbox) => "Running VirtualBox's own uninstaller (macOS asks for your password)…".into(),
        Note::Removing(Machine::VmwareUtility) => "Removing the Vagrant VMware Utility (macOS asks for your password)…".into(),
        Note::Removing(dep) => format!("Removing {} (macOS asks for your password)…", name(dep)),
        _ => String::new(),
    }
}

/// Why one of isothrum-machine's steps failed, in the player's words.
pub(super) fn step_failed(failed: isothrum_machine::install::StepFailed) -> Error {
    use isothrum_machine::install::StepFailed;
    match failed {
        StepFailed::NoPkexec => Error::Invalid(NO_PKEXEC.into()),
        StepFailed::Missing { program } => Error::Invalid(format!("{program} is not available on this machine.")),
        StepFailed::PromptRefused => {
            Error::Invalid("The password prompt was cancelled or the password was refused, so nothing was changed. Try again and enter your password.".into())
        }
        StepFailed::NoPolkitAgent => {
            Error::Invalid("No password prompt could be shown (no polkit agent is running). Run the command above in a terminal instead.".into())
        }
        StepFailed::SudoRefused => Error::Refused(SUDO_REFUSED.into()),
        StepFailed::Homebrew(trouble) => Error::Invalid(
            match trouble {
                isothrum_machine::install::brew::Trouble::Locked => BREW_LOCKED,
                isothrum_machine::install::brew::Trouble::Outdated => BREW_OUTDATED,
                isothrum_machine::install::brew::Trouble::XcodeTooOld => BREW_XCODE_OUTDATED,
            }
            .into(),
        ),
        StepFailed::Exited { program, code: Some(code) } => Error::Invalid(format!("`{program}` exited with exit status: {code}")),
        StepFailed::Exited { program, code: None } => Error::Invalid(format!("`{program}` was ended by a signal")),
        other => Error::Io(std::io::Error::other(other.to_string())),
    }
}

/// Runs `steps` in order, each announced as `$ program args` unless it has its own note.
pub async fn run_steps(steps: &[Step], on_line: &mut impl FnMut(String)) -> Result<()> {
    for step in steps {
        if step.note.is_none() {
            on_line(command_line(step));
        }
        run_step(step, on_line).await?;
    }
    Ok(())
}

/// How a step shows in the log.
pub fn command_line(step: &Step) -> String {
    // Redacted: the setup log is shown and copied, and a root install's proxy can carry a password.
    crate::platform::redact::redact(&format!("$ {} {}", step.program, step.args.join(" "))).into_owned()
}

/// Runs one install or removal step. Whatever it changed, the tools' kept answers (versions,
/// Vagrant's plugins) are forgotten before and after it, so the next machine check asks them
/// again: a check while the step ran (the Machine page checks whenever it is shown) would
/// otherwise keep what it read then, the plugin not installed yet, for minutes after the install.
pub async fn run_step(step: &Step, on_line: &mut impl FnMut(String)) -> Result<()> {
    crate::platform::remembered::forget_all();
    let ran = run_step_once(step, on_line).await;
    crate::platform::remembered::forget_all();
    ran
}

/// [`run_step`] itself.
async fn run_step_once(step: &Step, on_line: &mut impl FnMut(String)) -> Result<()> {
    if let Some(note) = &step.note {
        on_line(note.clone());
    }
    #[cfg(test)]
    if let Some(answer) = crate::platform::exec::fake::answer_step(&step.program, &step.args, on_line) {
        return answer;
    }
    let mut cmd = Command::new(&step.program);
    cmd.args(&step.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Installers that need a console of their own (winget, an .msi) still get none here: their
    // output streams into the app's setup log instead of a window flashing on screen.
    crate::platform::exec::headless(&mut cmd);
    // winget's own lines are read (the installer's exit code): in the C locale's words.
    crate::platform::exec::in_c_locale(&mut cmd);
    // In a group of its own, like every tool: a step whose output fails mid-way is ended with
    // what it started, not left installing unattended (#220).
    let (mut child, group) = crate::platform::exec::spawn_group(&mut cmd, true).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound if step.program == "pkexec" => Error::Invalid(NO_PKEXEC.into()),
        std::io::ErrorKind::NotFound => Error::Invalid(format!("{} is not available on this machine.", step.program)),
        _ => Error::Io(e),
    })?;
    // Both callbacks feed it; a Mutex (never contended: they run one after the other) keeps the
    // command's future Send.
    let watch = std::sync::Mutex::new(Watch::new(step));
    let watched = || watch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let forwarded = crate::platform::exec::forward_lines(
        &mut child,
        0,
        |line: String| {
            watched().line(&line);
            on_line(line);
        },
        |_: &str| {},
    )
    .await;
    if forwarded.is_ok() {
        group.disarm();
    }
    let (status, _) = forwarded?;
    if let Some(note) = watch.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner).end(step, status)? {
        on_line(note.into());
    }
    Ok(())
}

/// What a step's output says beyond its exit status, watched line by line for the program it
/// runs: the exit code of the installer winget ran (whether the player cancelled it).
struct Watch {
    /// The program the step runs, as far as its output is read.
    program: Program,
    installer_code: Option<u32>,
}

impl Watch {
    fn new(step: &Step) -> Watch {
        Watch { program: Program::of(step), installer_code: None }
    }

    /// A line of the step's output, either stream.
    fn line(&mut self, line: &str) {
        if self.program == Program::Winget {
            self.installer_code = exit_codes::installer_exit_code(line).or(self.installer_code);
        }
    }

    /// How the step ended, once it exited with `status`: done, with a note for the log (winget's
    /// "already installed", "restart to finish"), or why it failed.
    fn end(self, step: &Step, status: std::process::ExitStatus) -> Result<Option<&'static str>> {
        let verb = if step.args.first().is_some_and(|a| a == "uninstall") { Verb::Remove } else { Verb::Install };
        match end_of(&step.program, verb, status.code(), self.installer_code) {
            End::Done(note) => Ok(note),
            // A known code reads as what happened, with the raw codes after.
            End::Failed(why) => Err(Error::Invalid(match why {
                Some(why) => format!("{why} (`{}` exited with {status})", step.program),
                None => format!("`{}` exited with {status}", step.program),
            })),
        }
    }
}

/// The programs whose output a step's watch reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Program {
    /// winget: the installer it ran exits with a code of its own.
    Winget,
    /// Any other.
    Other,
}

impl Program {
    /// The program `step` runs.
    fn of(step: &Step) -> Program {
        match step.program.as_str() {
            "winget" => Program::Winget,
            _ => Program::Other,
        }
    }
}

/// How a step that exited with `code` ended.
#[derive(Debug, PartialEq)]
enum End {
    /// Done, with a note for the log (winget's "already installed", "restart to finish").
    Done(Option<&'static str>),
    /// Failed, with what that means when the code is known.
    Failed(Option<&'static str>),
}

/// What `program`'s exit `code` means for its step: winget's own codes, and an installer run
/// through PowerShell (WSL's) exiting with the installer's code, worded for the step's `verb`.
fn end_of(program: &str, verb: Verb, code: Option<i32>, installer_code: Option<u32>) -> End {
    if code == Some(0) {
        return End::Done(None);
    }
    match program {
        "winget" => match exit_codes::winget_done(code, verb) {
            Some(note) => End::Done(Some(note)),
            None => End::Failed(exit_codes::winget_failure(code, installer_code, verb)),
        },
        "powershell" => End::Failed(exit_codes::installer_failure(code, verb)),
        _ => End::Failed(None),
    }
}

#[cfg(test)]
mod tests;
