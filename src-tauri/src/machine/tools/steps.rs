//! Running install and removal steps: one program each, its output streamed to the setup log,
//! pkexec's own refusals told apart from the script's failures.

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
    /// A private folder of files the step reads (a root step's proxy settings), removed with
    /// the step.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code, reason = "only a Linux root step keeps a private folder"))]
    pub private: Option<tempfile::TempDir>,
}

/// A step that runs `program` with `args`, with no note.
pub fn step(program: impl Into<String>, args: &[&str]) -> Step {
    Step { program: program.into(), args: args.iter().map(ToString::to_string).collect(), note: None, private: None }
}

/// Without pkexec there is no graphical password prompt (Debian ships it apart from polkit).
pub const NO_PKEXEC: &str =
    "pkexec isn't installed, so Cyber CTF can't ask for your password. Install it once in a terminal (sudo apt install pkexec), then try again.";

/// A line pkexec prints when it, not the command, failed: refused ("Error executing command as
/// another user: Not authorized"), or no agent to ask ("…: No authentication agent found.", or,
/// with no terminal either, "Error creating textual authentication agent: …").
fn is_pkexec_error(line: &str) -> bool {
    line.starts_with("Error executing command as another user") || line.starts_with("Error creating textual authentication agent")
}

/// An unattended root step sudo refused to run without a password: nothing was changed.
pub const SUDO_REFUSED: &str = "sudo needs a password to act as an administrator here, and none can be asked for unattended, so nothing was changed. Run it as root, or on a terminal without --yes.";

/// What to tell the player when pkexec itself refused to run the command.
fn pkexec_refusal(stderr: &str) -> &'static str {
    if stderr.contains("No authentication agent") || stderr.contains("textual authentication agent") {
        "No password prompt could be shown (no polkit agent is running). Run the command above in a terminal instead."
    } else {
        "The password prompt was cancelled or the password was refused, so nothing was changed. Try again and enter your password."
    }
}

/// A Homebrew that can't update because a lock file is owned by another user (usually left by a
/// `sudo brew`), or a stale lock it thinks another update holds.
const BREW_LOCKED: &str = "Homebrew can't update because a lock file is owned by another user (this happens after running brew with sudo). Open the Terminal app and run: sudo chown -R $(whoami) \"$(brew --prefix)/var/homebrew\". Then try the install again here.";
/// A Homebrew too old to know this macOS ("unknown or unsupported macOS version: :dunno").
const BREW_OUTDATED: &str =
    "Homebrew is out of date for this version of macOS, so it can't install this. Open the Terminal app and run: brew update. Then try the install again here.";
/// A source build Homebrew refuses because the installed Xcode is older than it needs.
const BREW_XCODE_OUTDATED: &str = "Homebrew needs a newer Xcode than the one installed to build this. Update Xcode from the App Store (or delete /Applications/Xcode.app to use the lighter Command Line Tools). Then try the install again here.";

/// A broken-Homebrew condition read from brew's own error output, with how to fix it. These are
/// the player's environment to repair (an outdated Homebrew or Xcode, a lock owned by root), not
/// something the installer can mend on its own, so the step says what to do instead of just its
/// exit code.
fn brew_trouble(line: &str) -> Option<&'static str> {
    // A lock brew can't take: a root-owned or stale update lock (often after a `sudo brew`).
    if (line.contains("homebrew/locks") && line.contains("Permission denied")) || (line.contains("brew update") && line.contains("already running")) {
        Some(BREW_LOCKED)
    // Homebrew too old to recognise this macOS, so it has no bottle rule and refuses outright.
    } else if line.contains("unknown or unsupported macOS version") {
        Some(BREW_OUTDATED)
    // A build from source blocked by an Xcode older than Homebrew needs.
    } else if line.contains("is too outdated") || line.contains("Please update to Xcode") {
        Some(BREW_XCODE_OUTDATED)
    } else {
        None
    }
}

/// Whether this step drives Homebrew (directly, or through a shell line that calls it), so its
/// output is worth reading for a broken-Homebrew cause. apt, winget and pkexec never are.
fn is_brew_step(step: &Step) -> bool {
    step.program.contains("brew") || step.args.iter().any(|a| a.contains("brew"))
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
        |line| watched().error_line(line),
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
/// runs: the exit code of the installer winget ran (whether the player cancelled it), pkexec's
/// own refusal (told apart from the script's exit code), and a broken-Homebrew cause in brew's
/// output (the first one seen: an update's lock error comes before the install error it leads
/// to, so it wins; shown only if the step fails).
struct Watch {
    /// The program the step runs, as far as its output is read.
    program: Program,
    brew: bool,
    installer_code: Option<u32>,
    pkexec_error: Option<String>,
    /// sudo's refusal to run without a password, when it said so.
    sudo_refused: bool,
    brew_help: Option<&'static str>,
}

impl Watch {
    fn new(step: &Step) -> Watch {
        Watch { program: Program::of(step), brew: is_brew_step(step), installer_code: None, pkexec_error: None, sudo_refused: false, brew_help: None }
    }

    /// A line of the step's output, either stream.
    fn line(&mut self, line: &str) {
        if self.program == Program::Winget {
            self.installer_code = exit_codes::installer_exit_code(line).or(self.installer_code);
        }
    }

    /// A line of the step's error output.
    fn error_line(&mut self, line: &str) {
        if self.program == Program::Pkexec && is_pkexec_error(line) {
            self.pkexec_error = Some(line.to_string());
        }
        if self.program == Program::UnattendedSudo && line.starts_with("sudo: ") && line.contains("password") {
            self.sudo_refused = true;
        }
        if self.brew && self.brew_help.is_none() {
            self.brew_help = brew_trouble(line);
        }
    }

    /// How the step ended, once it exited with `status`: done, with a note for the log (winget's
    /// "already installed", "restart to finish"), or why it failed.
    fn end(self, step: &Step, status: std::process::ExitStatus) -> Result<Option<&'static str>> {
        // pkexec exits 126 when the password prompt was dismissed or refused and 127 when no
        // polkit agent could show one, but the script's own 126/127 (a command missing or not
        // executable) come through as is: only pkexec's own message says which it was.
        if let Some(why) = self.pkexec_error.filter(|_| matches!(status.code(), Some(126 | 127))) {
            return Err(Error::Invalid(pkexec_refusal(&why).into()));
        }
        if self.sudo_refused && !status.success() {
            return Err(Error::Refused(SUDO_REFUSED.into()));
        }
        let verb = if step.args.first().is_some_and(|a| a == "uninstall") { Verb::Remove } else { Verb::Install };
        match end_of(&step.program, verb, status.code(), self.installer_code) {
            End::Done(note) => Ok(note),
            // A recognised broken-Homebrew cause reads as the fix for it; the raw brew lines stay
            // in the log. Otherwise a known code reads as what happened, with the raw codes after.
            End::Failed(why) => Err(Error::Invalid(match self.brew_help {
                Some(help) => help.to_string(),
                None => match why {
                    Some(why) => format!("{why} (`{}` exited with {status})", step.program),
                    None => format!("`{}` exited with {status}", step.program),
                },
            })),
        }
    }
}

/// The programs whose output a step's watch reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Program {
    /// winget: the installer it ran exits with a code of its own.
    Winget,
    /// pkexec: its own refusals, told apart from the script's.
    Pkexec,
    /// `sudo -n` (an unattended root step): its refusal to run without a password.
    UnattendedSudo,
    /// Any other.
    Other,
}

impl Program {
    /// The program `step` runs.
    fn of(step: &Step) -> Program {
        match step.program.as_str() {
            "winget" => Program::Winget,
            "pkexec" => Program::Pkexec,
            "sudo" if step.args.first().is_some_and(|a| a == "-n") => Program::UnattendedSudo,
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
