//! Running install and removal steps: one program each, its output streamed to the setup log,
//! pkexec's own refusals told apart from the script's failures.

use std::process::Stdio;

use tokio::process::Command;

use super::exit_codes;
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

/// A line pkexec prints when it, not the command, failed: refused ("Error executing command as
/// another user: Not authorized"), or no agent to ask ("…: No authentication agent found.", or,
/// with no terminal either, "Error creating textual authentication agent: …").
fn is_pkexec_error(line: &str) -> bool {
    line.starts_with("Error executing command as another user") || line.starts_with("Error creating textual authentication agent")
}

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
    winget: bool,
    pkexec: bool,
    brew: bool,
    installer_code: Option<u32>,
    pkexec_error: Option<String>,
    brew_help: Option<&'static str>,
}

impl Watch {
    fn new(step: &Step) -> Watch {
        Watch {
            winget: step.program == "winget",
            pkexec: step.program == "pkexec",
            brew: is_brew_step(step),
            installer_code: None,
            pkexec_error: None,
            brew_help: None,
        }
    }

    /// A line of the step's output, either stream.
    fn line(&mut self, line: &str) {
        if self.winget {
            self.installer_code = exit_codes::installer_exit_code(line).or(self.installer_code);
        }
    }

    /// A line of the step's error output.
    fn error_line(&mut self, line: &str) {
        if self.pkexec && is_pkexec_error(line) {
            self.pkexec_error = Some(line.to_string());
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
        let removing = step.args.first().is_some_and(|a| a == "uninstall");
        match end_of(&step.program, removing, status.code(), self.installer_code) {
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

/// How a step that exited with `code` ended.
#[derive(Debug, PartialEq)]
enum End {
    /// Done, with a note for the log (winget's "already installed", "restart to finish").
    Done(Option<&'static str>),
    /// Failed, with what that means when the code is known.
    Failed(Option<&'static str>),
}

/// What `program`'s exit `code` means for its step: winget's own codes, and an installer run
/// through PowerShell (WSL's) exiting with the installer's code. `removing`: an uninstall.
fn end_of(program: &str, removing: bool, code: Option<i32>, installer_code: Option<u32>) -> End {
    if code == Some(0) {
        return End::Done(None);
    }
    match program {
        "winget" => match exit_codes::winget_done(code, removing) {
            Some(note) => End::Done(Some(note)),
            None => End::Failed(exit_codes::winget_failure(code, installer_code, removing)),
        },
        "powershell" => End::Failed(exit_codes::installer_failure(code, removing)),
        _ => End::Failed(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn what_a_check_read_while_a_step_ran_is_forgotten_once_it_ends() {
        // Regression: the kept answers were forgotten only before the step, so a machine check
        // during a `vagrant plugin install` kept the plugin list without the plugin, and the
        // finished install still read as "needs setup" for minutes.
        use crate::platform::remembered::{is_kept, keep};
        let key = format!("vagrant plugin list {}", rand::random::<u32>());
        let during = key.clone();
        let _tools = crate::platform::exec::fake::Tools::new(move |_, _| {
            keep(&during, "vagrant-qemu (0.8.0, global)");
            crate::platform::exec::fake::ok("Installed the plugin 'vagrant-vmware-desktop (3.0.5)'!")
        });
        run_step(&step("vagrant", &["plugin", "install", "vagrant-vmware-desktop"]), &mut |_| {}).await.unwrap();
        assert!(!is_kept(&key), "the list read mid-install still stands");
    }

    /// An exit status with `code`, as the OS reports it.
    #[cfg(unix)]
    fn exited(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(unix)]
    #[test]
    fn each_program_is_watched_for_what_its_own_output_says() {
        // pkexec: its refusal, only with its own exit codes; a script's lines aren't watched.
        let pkexec = step("pkexec", &["sh", "-c", "apt-get install -y x"]);
        let mut w = Watch::new(&pkexec);
        w.error_line("Error executing command as another user: Not authorized");
        assert!(w.end(&pkexec, exited(126)).unwrap_err().to_string().contains("cancelled"));
        let mut w = Watch::new(&step("sh", &["-c", "x"]));
        w.error_line("Error executing command as another user: Not authorized");
        assert_eq!(w.pkexec_error, None);
        // brew: the first broken-Homebrew cause, said when the step fails, not when it succeeds.
        let brew = step("brew", &["install", "qemu"]);
        let mut w = Watch::new(&brew);
        w.error_line("/opt/homebrew/var/homebrew/locks/update: Permission denied");
        w.error_line("Error: unknown or unsupported macOS version");
        assert_eq!(w.brew_help, Some(BREW_LOCKED));
        assert_eq!(w.end(&brew, exited(1)).unwrap_err().to_string(), BREW_LOCKED);
        let mut w = Watch::new(&brew);
        w.error_line("Error: unknown or unsupported macOS version");
        assert_eq!(w.end(&brew, exited(0)).unwrap(), None);
        // winget: the installer's code from any line of its output; other programs' lines aren't read.
        let mut w = Watch::new(&step("winget", &["install", "-e", "--id", "Docker.DockerDesktop"]));
        w.line("Installer failed with exit code: 1602");
        assert_eq!(w.installer_code, Some(1602));
        let mut w = Watch::new(&step("powershell", &["-Command", "x"]));
        w.line("Installer failed with exit code: 1602");
        assert_eq!(w.installer_code, None);
    }

    #[test]
    fn pkexec_refusals_name_their_cause() {
        // pkexec's own lines (polkit 126), as it prints them.
        let refused = "Error executing command as another user: Not authorized";
        let no_agent = "Error executing command as another user: No authentication agent found.";
        let no_tty =
            "Error creating textual authentication agent: Error opening current controlling terminal for the process (`/dev/tty'): No such device or address";
        assert!(is_pkexec_error(refused) && is_pkexec_error(no_agent) && is_pkexec_error(no_tty));
        assert!(pkexec_refusal(refused).contains("cancelled"));
        assert!(pkexec_refusal(no_agent).contains("polkit agent") && pkexec_refusal(no_tty).contains("polkit agent"));
        // The script's own output never passes for pkexec's.
        assert!(!is_pkexec_error("sh: 1: gpg: not found"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn steps_stream_both_outputs_and_report_a_failure() {
        let mut lines = Vec::new();
        let ok = Step { program: "sh".into(), args: vec!["-c".into(), "echo out; echo err >&2".into()], note: Some("Note first".into()) };
        let fail = step("sh", &["-c", "exit 3"]);
        let err = run_steps(&[ok, fail], &mut |l| lines.push(l)).await.unwrap_err();
        assert_eq!(lines[0], "Note first");
        assert!(lines.contains(&"out".to_string()) && lines.contains(&"err".to_string()));
        assert_eq!(lines.last().map(String::as_str), Some("$ sh -c exit 3"));
        assert!(err.to_string().starts_with("`sh` exited with"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_script_exiting_126_or_127_with_pkexecs_words_is_still_its_own_failure() {
        // Only pkexec's own refusal (126/127 *and* its message, from pkexec itself) reads as the
        // prompt being cancelled; a script that prints the same words and exits 126 is a plain
        // failure of that program, raw code kept.
        let mut lines = Vec::new();
        let s = step("sh", &["-c", "echo 'Error executing command as another user: Not authorized' >&2; exit 126"]);
        let err = run_step(&s, &mut |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.to_string(), "`sh` exited with exit status: 126");
        assert_eq!(lines, ["Error executing command as another user: Not authorized"]);
        let err = run_step(&step("sh", &["-c", "exit 127"]), &mut |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "`sh` exited with exit status: 127");
    }

    #[test]
    fn a_winget_step_ends_by_winget_s_codes() {
        let winget = |code: u32, installer| end_of("winget", false, Some(code.cast_signed()), installer);
        assert_eq!(end_of("winget", false, Some(0), None), End::Done(None));
        // Regression (#220): nothing to do, or a restart to finish, is a step done.
        assert!(matches!(winget(0x8A15_002B, None), End::Done(Some(note)) if note.starts_with("Already installed")));
        assert!(matches!(winget(0x8A15_0109, None), End::Done(Some(note)) if note.contains("Restart Windows")));
        assert!(matches!(winget(0x8A15_0006, Some(1602)), End::Failed(Some(why)) if why.contains("cancelled")));
        assert_eq!(winget(1, None), End::Failed(None));
    }

    #[test]
    fn another_step_ends_by_its_own_codes() {
        // Only winget's codes are winget's: another program exiting with one just failed.
        assert_eq!(end_of("sh", false, Some(0x8A15_002B_u32.cast_signed()), None), End::Failed(None));
        assert!(matches!(end_of("powershell", true, Some(1223), None), End::Failed(Some(why)) if why.starts_with("The removal was cancelled")));
        assert_eq!(end_of("pkexec", false, None, None), End::Failed(None), "killed by a signal");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_step_whose_output_fails_takes_what_it_started_with_it() {
        // Regression (#220): an install step was spawned without kill_on_drop or a group, so a
        // failed read of its output left it (and its installer) running unattended.
        let file = std::env::temp_dir().join(format!("cyberctf-step-tree-{}", rand::random::<u32>()));
        let script = format!("sleep 120 & echo $! > '{}'; wait", file.display());
        let started = tokio::time::timeout(std::time::Duration::from_secs(2), run_step(&step("sh", &["-c", &script]), &mut |_| {})).await;
        assert!(started.is_err(), "cancelled while it ran");
        let pid: u32 = std::fs::read_to_string(&file).expect("the step started its child").trim().parse().unwrap();
        let _ = std::fs::remove_file(&file);
        assert!(crate::platform::exec::fake::gone(pid).await, "the step's child {pid} was left running");
    }

    #[test]
    fn brew_trouble_names_a_broken_homebrew_and_its_fix() {
        assert_eq!(brew_trouble("/opt/homebrew/var/homebrew/locks/update: Permission denied"), Some(BREW_LOCKED));
        assert_eq!(brew_trouble("Error: Another `brew update` process is already running."), Some(BREW_LOCKED));
        assert_eq!(brew_trouble("Error: unknown or unsupported macOS version: :dunno"), Some(BREW_OUTDATED));
        assert_eq!(brew_trouble("Error: Your Xcode (26.6) at /Applications/Xcode.app is too outdated."), Some(BREW_XCODE_OUTDATED));
        assert_eq!(brew_trouble("Please update to Xcode 27.0 (or delete it)."), Some(BREW_XCODE_OUTDATED));
        // Ordinary brew progress isn't a fault.
        assert_eq!(brew_trouble("==> Fetching qemu"), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_brew_step_surfaces_the_homebrew_fix_not_the_exit_code() {
        // A brew install (its command line mentions brew) that dies on an outdated Xcode reports
        // the fix, not the bare exit code; the raw brew line is still streamed to the log.
        let mut lines = Vec::new();
        let s = step("sh", &["-c", "echo 'Error: Your Xcode (26.6) is too outdated.' >&2; exit 1  # brew install qemu"]);
        let err = run_step(&s, &mut |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.to_string(), BREW_XCODE_OUTDATED);
        assert!(lines.iter().any(|l| l.contains("is too outdated")), "the raw brew output is still shown: {lines:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_non_brew_step_is_not_read_for_homebrew_trouble() {
        // The same words from apt or a lab are not reinterpreted: only brew steps are read.
        let err = run_step(&step("sh", &["-c", "echo 'is too outdated' >&2; exit 1"]), &mut |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "`sh` exited with exit status: 1");
    }

    #[tokio::test]
    async fn a_missing_program_says_so() {
        let err = run_step(&step("cyberctf-no-such-tool", &[]), &mut |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "cyberctf-no-such-tool is not available on this machine.");
    }

    #[test]
    fn a_command_line_reads_as_typed() {
        assert_eq!(command_line(&step("brew", &["install", "qemu"])), "$ brew install qemu");
        // A proxy's password never reaches the setup log.
        let root = step("pkexec", &["env", "https_proxy=http://ada:s3cret@proxy.corp:3128", "sh", "-c", "apt-get update"]);
        assert_eq!(command_line(&root), "$ pkexec env https_proxy=http://ada:[redacted]@proxy.corp:3128 sh -c apt-get update");
    }
}
