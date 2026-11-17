//! Running install and removal steps: isothrum-machine's ([`isothrum_machine::install`]), each
//! one program, its output streamed to the setup log; what each is, how it ended and why it failed
//! said here in the player's words.

use isothrum_machine::install::exit_codes::{Done, Known};
use isothrum_machine::install::{NotServed, Note, Step, StepFailed};

use crate::error::{Code, Error, Result};

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

/// An install's or a removal's steps.
pub type Plan = Vec<Step>;

/// Runs `plan`'s steps in order, each announced as `$ program args` (redacted) unless it says what
/// it is ([`said`]).
pub async fn run_plan(plan: &[Step], on_line: &mut impl FnMut(String)) -> Result<()> {
    for step in plan {
        on_line(match step.note {
            Some(note) => said(note),
            None => crate::platform::redact::redact(&isothrum_machine::install::command_line(step)).into_owned(),
        });
        run_step(step, on_line).await?;
    }
    Ok(())
}

/// Runs one step through the app's runner. Whatever it changed, the tools' kept answers (versions,
/// Vagrant's plugins) are forgotten before and after it, so the next machine check asks them
/// again: a check while the step ran (the Machine page checks whenever it is shown) would
/// otherwise keep what it read then, the plugin not installed yet, for minutes after the install.
pub async fn run_step(step: &Step, on_line: &mut impl FnMut(String)) -> Result<()> {
    crate::platform::remembered::forget_all();
    let cx = crate::runtime::providers::machine_setup().await;
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
    let removal = is_removal(step);
    if let Some(done) = ran.map_err(|failed| step_failed(failed, removal))? {
        on_line(done_said(done, removal).into());
    }
    Ok(())
}

/// Whether `step` removes its tool (`winget uninstall …`), for the words of its outcome.
fn is_removal(step: &Step) -> bool {
    step.args.first().is_some_and(|a| a == "uninstall")
}

/// What a step is, in the player's words, said before it instead of its command line.
fn said(note: isothrum_machine::install::Note) -> String {
    use isothrum_machine::install::Dependency as Machine;
    let name = |dep| super::install::name_on(dep, isothrum_machine::HostOs::Macos);
    match note {
        Note::Downloading(dep) => format!("Downloading {}…", name(dep)),
        Note::QemuUpdatingHomebrewFirst => "Installing QEMU (updating Homebrew first if it is out of date for this macOS).".into(),
        Note::TrustingAzureTap => "Trusting the azure/azure-cli tap (Homebrew 7 asks for it; older versions don't).".into(),
        Note::AzureCliCask => "Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into(),
        Note::DownloadingVmwareUtility { version } => format!("Downloading HashiCorp's Vagrant VMware Utility {version}…"),
        Note::Removing(Machine::Docker) => "Running Docker Desktop's own uninstaller…".into(),
        Note::Removing(Machine::Virtualbox) => "Running VirtualBox's own uninstaller (macOS asks for your password)…".into(),
        Note::Removing(Machine::VmwareUtility) if isothrum_runner::this_host() == isothrum_machine::HostOs::Windows => {
            "Removing the Vagrant VMware Utility (Windows asks for an administrator's approval)…".into()
        }
        Note::Removing(Machine::VmwareUtility) => "Removing the Vagrant VMware Utility (macOS asks for your password)…".into(),
        Note::Removing(dep) => format!("Removing {} (macOS asks for your password)…", name(dep)),
        _ => String::new(),
    }
}

/// A step done although its program exited non-zero, in the player's words.
fn done_said(done: Done, removal: bool) -> &'static str {
    match done {
        Done::AlreadyInstalled => "Already installed and up to date: nothing to do.",
        Done::RestartToFinish if removal => "Removed. Restart Windows to finish.",
        _ => "Installed. Restart Windows to finish.",
    }
}

/// What a failed step's codes say happened, in the player's words.
fn known_said(known: Known, removal: bool) -> &'static str {
    match known {
        Known::Cancelled if removal => "The removal was cancelled, so nothing was changed. Try again and accept the administrator prompt.",
        Known::Cancelled => "The installation was cancelled, so nothing was installed. Try again and accept the administrator prompt.",
        Known::AnotherInstall => "Another installation is already running. Wait for it to finish, then try again.",
        Known::NoDiskSpace => "There isn't enough disk space. Free up some space, then try again.",
        Known::NoInternet => "This needs an internet connection. Check your connection, then try again.",
        Known::RestartFirst => "Restart Windows, then try again.",
        Known::Policy => "This is blocked by a policy on this computer. Ask your administrator.",
        _ => "The installer reported an error, so it didn't finish. The details are in the log above.",
    }
}

/// Why a step failed, in the player's words (`removal`: a removal's, for a cancel's words).
pub(super) fn step_failed(failed: StepFailed, removal: bool) -> Error {
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
        StepFailed::NotServed(NotServed::DockerDistribution) => Error::coded(Code::DockerOtherDistribution),
        StepFailed::NotServed(NotServed::HypervEdition) => Error::coded(Code::HypervHomeEdition),
        StepFailed::Homebrew(trouble) => Error::Invalid(
            match trouble {
                isothrum_machine::install::brew::Trouble::Locked => BREW_LOCKED,
                isothrum_machine::install::brew::Trouble::Outdated => BREW_OUTDATED,
                isothrum_machine::install::brew::Trouble::XcodeTooOld => BREW_XCODE_OUTDATED,
            }
            .into(),
        ),
        StepFailed::Exited { program, code, known } => {
            let status = code.map_or_else(|| "a signal".to_owned(), |code| format!("exit status: {code}"));
            Error::Invalid(match known {
                Some(known) => format!("{} (`{program}` exited with {status})", known_said(known, removal)),
                None => format!("`{program}` exited with {status}"),
            })
        }
        other => Error::Io(std::io::Error::other(other.to_string())),
    }
}

#[cfg(test)]
mod tests;
