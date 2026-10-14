//! Running install and removal steps: one program each, its output streamed to the setup log,
//! pkexec's own refusals told apart from the script's failures.

use std::process::Stdio;

use tauri::ipc::Channel;
use tokio::process::Command;

use super::exit_codes;
use crate::error::{Error, Result};

/// One command of an install or removal.
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
    /// A note shown before the step (e.g. when we open a download instead of installing).
    pub note: Option<String>,
}

pub fn step(program: impl Into<String>, args: &[&str]) -> Step {
    Step { program: program.into(), args: args.iter().map(|s| s.to_string()).collect(), note: None }
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

/// Sends each line to the UI's log channel.
pub fn channel_log(logs: Channel<String>) -> impl FnMut(String) {
    move |line: String| {
        let _ = logs.send(line);
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
    format!("$ {} {}", step.program, step.args.join(" "))
}

pub async fn run_step(step: &Step, on_line: &mut impl FnMut(String)) -> Result<()> {
    if let Some(note) = &step.note {
        on_line(note.clone());
    }
    #[cfg(test)]
    if let Some(answer) = crate::exec::fake::answer_step(&step.program, &step.args, on_line) {
        return answer;
    }
    let mut cmd = Command::new(&step.program);
    cmd.args(&step.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Installers that need a console of their own (winget, an .msi) still get none here: their
    // output streams into the app's setup log instead of a window flashing on screen.
    crate::exec::headless(&mut cmd);
    // In a group of its own, like every tool: a step whose output fails mid-way is ended with
    // what it started, not left installing unattended (#220).
    let (mut child, group) = crate::exec::spawn_group(&mut cmd, true).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound if step.program == "pkexec" => Error::Invalid(NO_PKEXEC.into()),
        std::io::ErrorKind::NotFound => Error::Invalid(format!("{} is not available on this machine.", step.program)),
        _ => Error::Io(e),
    })?;
    // pkexec's own failure (refused, or no agent to ask), told apart from the script's exit code.
    let mut pkexec_error: Option<String> = None;
    // The exit code of the installer winget ran, which says whether the player cancelled it.
    let mut installer_code: Option<u32> = None;
    let watch = |line: String| {
        if step.program == "winget" {
            installer_code = exit_codes::installer_exit_code(&line).or(installer_code);
        }
        on_line(line);
    };
    let forwarded = crate::exec::forward_lines(&mut child, 0, watch, |line| {
        if step.program == "pkexec" && is_pkexec_error(line) {
            pkexec_error = Some(line.to_string());
        }
    })
    .await;
    if forwarded.is_ok() {
        group.disarm();
    }
    let (status, _) = forwarded?;
    // pkexec exits 126 when the password prompt was dismissed or refused and 127 when no polkit
    // agent could show one, but the script's own 126/127 (a command missing or not executable)
    // come through as is: only pkexec's own message says which it was.
    if let Some(why) = pkexec_error.filter(|_| matches!(status.code(), Some(126 | 127))) {
        return Err(Error::Invalid(pkexec_refusal(&why).into()));
    }
    let removing = step.args.first().is_some_and(|a| a == "uninstall");
    match end_of(&step.program, removing, status.code(), installer_code) {
        End::Done(note) => {
            if let Some(note) = note {
                on_line(note.into());
            }
            Ok(())
        }
        // A known code reads as what happened; the raw codes follow it (and are in the log).
        End::Failed(Some(why)) => Err(Error::Invalid(format!("{why} (`{}` exited with {status})", step.program))),
        End::Failed(None) => Err(Error::Invalid(format!("`{}` exited with {status}", step.program))),
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
    fn a_step_ends_by_its_programs_codes() {
        let winget = |code: u32, installer| end_of("winget", false, Some(code as i32), installer);
        assert_eq!(end_of("winget", false, Some(0), None), End::Done(None));
        // Regression (#220): nothing to do, or a restart to finish, is a step done.
        assert!(matches!(winget(0x8A15_002B, None), End::Done(Some(note)) if note.starts_with("Already installed")));
        assert!(matches!(winget(0x8A15_0109, None), End::Done(Some(note)) if note.contains("Restart Windows")));
        assert!(matches!(winget(0x8A15_0006, Some(1602)), End::Failed(Some(why)) if why.contains("cancelled")));
        assert_eq!(winget(1, None), End::Failed(None));
        // Only winget's codes are winget's: another program exiting with one just failed.
        assert_eq!(end_of("sh", false, Some(0x8A15_002B_u32 as i32), None), End::Failed(None));
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
        assert!(crate::exec::fake::gone(pid).await, "the step's child {pid} was left running");
    }

    #[tokio::test]
    async fn a_missing_program_says_so() {
        let err = run_step(&step("cyberctf-no-such-tool", &[]), &mut |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "cyberctf-no-such-tool is not available on this machine.");
    }

    #[test]
    fn channel_log_sends_each_line_to_the_ui() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let channel: Channel<String> = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                sink.lock().unwrap().push(serde_json::from_str::<String>(&json).unwrap());
            }
            Ok(())
        });
        let mut log = channel_log(channel);
        log("one".into());
        log("two".into());
        assert_eq!(seen.lock().unwrap().as_slice(), ["one", "two"]);
        assert_eq!(command_line(&step("brew", &["install", "qemu"])), "$ brew install qemu");
    }
}
