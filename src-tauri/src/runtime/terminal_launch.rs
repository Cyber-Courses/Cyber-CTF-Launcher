//! Opening the OS terminal on the attack box shell: the runner builds the command (program,
//! arguments and the run's folder — see [`isothrum_runner::Running::console_command`]); this turns
//! it into a command line and hands it to the platform's terminal. The in-app pseudo-terminal
//! ([`crate::terminal`]) runs the same command without a terminal window; this is the "open in a
//! system terminal" path, so the player gets a real terminal they fully control.

use isothrum_runner::ShellCommand;

use crate::error::{Error, Result};

/// The POSIX command line that runs `command` in its folder: `cd <cwd> && exec <program> <args>`
/// (the `exec` so leaving the shell ends it, no extra process in between), through `env` with
/// the variables the command needs (a Vagrant run's state folder), when it has any. Unix
/// terminals only (Windows uses `windows_line`); the tests exercise it on every platform.
#[cfg(any(unix, test))]
/// How long a terminal gets to die on its own (one that can't start, zutty without its font,
/// does so within a moment) before it counts as started.
const TERMINAL_START_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

fn command_line(command: &ShellCommand) -> String {
    let mut words = Vec::new();
    if !command.env.is_empty() {
        words.push("env".to_owned());
        words.extend(command.env.iter().map(|(name, value)| format!("{name}={value}")));
    }
    words.push(command.program.clone());
    words.extend(command.args.iter().cloned());
    let run = shell_words::join(&words);
    format!("cd {} && exec {run}", shell_words::quote(&command.cwd.to_string_lossy()))
}

/// [`open_terminal`] off the async runtime's threads and the UI thread: finding a terminal that
/// starts can take a few seconds (each candidate gets a moment to fail), which froze the window
/// when a command ran it on the main thread.
pub async fn open_terminal_async(command: ShellCommand) -> Result<()> {
    tokio::task::spawn_blocking(move || open_terminal(&command)).await.map_err(|e| Error::Invalid(format!("couldn't open a terminal: {e}")))?
}

/// Escapes text for an AppleScript double-quoted string.
#[cfg(any(target_os = "macos", test))]
fn applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Opens the OS terminal on `command`, titled "Cyber CTF attack box". The command line itself
/// never stays on screen: the window is cleared first, and the command replaces the shell
/// (`exec`), so leaving the box ends the session. `DOCKER_CLI_HINTS=false` drops Docker's
/// "What's next" ad on exit.
pub fn open_terminal(command: &ShellCommand) -> Result<()> {
    #[cfg(unix)]
    let line = command_line(command);
    #[cfg(target_os = "macos")]
    {
        // `do script` types the line into a new window; escape it for the AppleScript string.
        // The leading space keeps it out of shell history where HIST_IGNORE_SPACE is on.
        let line = applescript_string(&format!(" clear; DOCKER_CLI_HINTS=false; {line}"));
        // Title: ours alone where AppleScript allows it (no size, device or shell path). The working
        // folder and the running command are Terminal profile settings it can't turn off.
        let script = format!(
            "tell application \"Terminal\"\nactivate\nset t to do script \"{line}\"\nset custom title of t to \"Cyber CTF attack box\"\n\
             set title displays custom title of t to true\nset title displays window size of t to false\n\
             set title displays device name of t to false\nset title displays shell path of t to false\n\
             set title displays file name of t to false\nend tell"
        );
        std::process::Command::new("osascript").arg("-e").arg(script).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        open_terminal_cmd(&windows_line(command))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let run = format!("clear; DOCKER_CLI_HINTS=false; {line}");
        // gnome-terminal's `-e` takes one string, so it gets `--`; the others run what follows `-e`.
        for (term, flag) in [("x-terminal-emulator", "-e"), ("gnome-terminal", "--"), ("konsole", "-e"), ("xterm", "-e")] {
            let Ok(mut child) = std::process::Command::new(term).args([flag, "sh", "-c", &run]).spawn() else { continue };
            // A terminal that can't start (e.g. zutty without its font) dies within a moment
            // without a window, so give it that moment and fall through to the next one.
            std::thread::sleep(TERMINAL_START_GRACE);
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => {}
                _ => {
                    // Reaped when it closes, or each terminal opened stays a zombie while the
                    // app runs (Debian's gnome-terminal wrapper waits for its window).
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(());
                }
            }
        }
        Err(Error::Invalid(format!("couldn't open a terminal; run this yourself: {line}")))
    }
}

/// The cmd.exe command line: `cd /d "<cwd>" && set "<VAR>=<value>" && <program> <args>`, each
/// word double-quoted, the command's variables set first.
#[cfg(any(windows, test))]
pub(crate) fn windows_line(command: &ShellCommand) -> String {
    let quote = |w: &str| if w.contains(' ') || w.contains('"') { format!("\"{}\"", w.replace('"', "\"\"")) } else { w.to_owned() };
    let mut run = std::iter::once(&command.program).chain(&command.args).map(|w| quote(w)).collect::<Vec<_>>().join(" ");
    for (name, value) in command.env.iter().rev() {
        run = format!("set \"{name}={value}\" && {run}");
    }
    format!("cd /d {} && {run}", quote(&command.cwd.to_string_lossy()))
}

/// Opens a console window titled "Cyber CTF attack box" on `line`, a cmd.exe command line. The
/// line goes into a temp .cmd that is run (cmd.exe's quote-stripping with nested quotes and `&`
/// is unreliable when a path has a space): the line lives in the file with normal double-quoted
/// paths, and the only thing cmd parses is the balanced-quoted .cmd path.
#[cfg(windows)]
pub fn open_terminal_cmd(line: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let bat = write_shell_script(&std::env::temp_dir(), line)?;
    std::process::Command::new("cmd")
        .raw_arg(format!("/c start \"Cyber CTF attack box\" cmd /c \"{}\"", bat.display()))
        .env("DOCKER_CLI_HINTS", "false")
        .spawn()?;
    Ok(())
}

/// The temp scripts the Windows terminals run: `cyberctf-shell-<unique>.cmd`.
#[cfg(windows)]
const SCRIPT_PREFIX: &str = "cyberctf-shell-";

/// How long a shell's script is kept: cmd has read the line it runs long before.
#[cfg(windows)]
const SCRIPT_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Writes the script a Windows terminal runs for `line` into `dir`, under a name no other shell
/// uses, and clears the scripts earlier shells left. The script deletes itself when its shell
/// ends; one whose window was closed mid-session is cleared by a later call.
#[cfg(windows)]
fn write_shell_script(dir: &std::path::Path, line: &str) -> Result<std::path::PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    clear_old_scripts(dir, SCRIPT_TTL);
    let nanos = crate::platform::clock::since_epoch().as_nanos();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{SCRIPT_PREFIX}{}-{n}-{nanos}.cmd", std::process::id()));
    std::fs::write(&path, shell_script(line))?;
    Ok(path)
}

/// A script running `line`: the screen cleared first, the script removed once it ends. A `%` in
/// a batch file starts a variable, so the line's are doubled to stay literal. The file is UTF-8
/// and cmd.exe reads each line in the console's code page as it reaches it: `chcp 65001` first,
/// or a path with "é" (C:\Users\José) arrives garbled.
#[cfg(windows)]
fn shell_script(line: &str) -> String {
    format!("@echo off\r\nchcp 65001 >nul\r\ncls\r\n{}\r\n(goto) 2>nul & del \"%~f0\"\r\n", line.replace('%', "%%"))
}

/// Removes the shell scripts in `dir` older than `ttl`.
#[cfg(windows)]
fn clear_old_scripts(dir: &std::path::Path, ttl: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(SCRIPT_PREFIX) && std::path::Path::new(&name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age >= ttl);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cmd(program: &str, args: &[&str], cwd: &str) -> ShellCommand {
        ShellCommand::new(program, args.iter().copied(), PathBuf::from(cwd))
    }

    /// The shell into a Vagrant run's box: `vagrant` needs the run's state folder to find the VM.
    fn in_vm(cwd: &str) -> ShellCommand {
        let mut c = cmd("vagrant", &["ssh", "default"], cwd);
        c.env = vec![("VAGRANT_DOTFILE_PATH".into(), "/runs/x y/vagrant".into())];
        c
    }

    #[test]
    fn the_lines_set_the_variables_the_command_needs() {
        assert_eq!(
            command_line(&in_vm("/labs/x/.isoloom/docker-vm")),
            "cd /labs/x/.isoloom/docker-vm && exec env 'VAGRANT_DOTFILE_PATH=/runs/x y/vagrant' vagrant ssh default"
        );
        assert_eq!(windows_line(&in_vm("C:\\labs\\x")), "cd /d C:\\labs\\x && set \"VAGRANT_DOTFILE_PATH=/runs/x y/vagrant\" && vagrant ssh default");
    }

    #[test]
    fn the_posix_line_runs_the_command_in_its_folder_and_execs_it() {
        let line = command_line(&cmd("docker", &["compose", "--file", "/runs/x y/compose.yml", "exec", "kali", "bash"], "/runs/x y"));
        assert_eq!(line, "cd '/runs/x y' && exec docker compose --file '/runs/x y/compose.yml' exec kali bash");
    }

    #[test]
    fn the_windows_line_quotes_paths_with_spaces() {
        assert_eq!(
            windows_line(&cmd("docker", &["compose", "exec", "kali", "bash"], "C:\\runs\\x y")),
            "cd /d \"C:\\runs\\x y\" && docker compose exec kali bash"
        );
    }

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string("a\"b\\c"), "a\\\"b\\\\c");
    }
}
