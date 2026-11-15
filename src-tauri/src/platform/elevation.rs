//! How a step that needs an administrator is elevated. The app (and the command line on a
//! terminal) asks the player: pkexec's password prompt on Linux, the macOS administrator prompt,
//! Windows' UAC prompt. Unattended (the command line with `--yes` and no terminal: a script, a
//! fresh machine set up from a test harness) nobody is there to answer a prompt, so none may be
//! raised: the process is an administrator already, or (Linux) sudo runs the step without asking
//! (`sudo -n`, refused at once rather than waiting on a password). [`check_unattended`] tells
//! whether elevation can happen before anything runs.

#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// Whether this process elevates unattended (see the module).
#[cfg(not(test))]
static UNATTENDED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Per test thread in unit tests: a command-line test going unattended leaves the install
    /// tests running beside it as the app (prompting).
    static UNATTENDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Elevates unattended from now on (`true`), or through the prompts (`false`, the default).
pub fn set_unattended(on: bool) {
    #[cfg(not(test))]
    UNATTENDED.store(on, Ordering::SeqCst);
    #[cfg(test)]
    UNATTENDED.with(|u| u.set(on));
}

/// Whether this process elevates unattended.
pub fn unattended() -> bool {
    #[cfg(not(test))]
    return UNATTENDED.load(Ordering::SeqCst);
    #[cfg(test)]
    return UNATTENDED.with(std::cell::Cell::get);
}

/// `program` with `args` run as root on Linux: through pkexec (its password prompt); unattended,
/// as is when this process is root already, else through `sudo -n`.
pub fn as_root(program: &str, args: &[String]) -> (String, Vec<String>) {
    as_root_with(program, args, unattended(), is_root())
}

/// [`as_root`], for an unattended process or not, root or not.
fn as_root_with(program: &str, args: &[String], unattended: bool, root: bool) -> (String, Vec<String>) {
    let all = |prefix: &[&str]| prefix.iter().map(ToString::to_string).chain(std::iter::once(program.to_owned())).chain(args.iter().cloned()).collect();
    match (unattended, root) {
        (false, _) => ("pkexec".into(), all(&[])),
        (true, true) => (program.to_owned(), args.to_vec()),
        (true, false) => ("sudo".into(), all(&["-n"])),
    }
}

/// Whether this process runs as root (an administrator, on Windows: elevated).
pub fn is_root() -> bool {
    #[cfg(unix)]
    return crate::platform::sys::is_root();
    #[cfg(windows)]
    return crate::platform::sys::is_elevated();
}

/// What an unattended elevation needs, said when it's missing.
const CANT_ELEVATE: &str = "Nothing can be installed unattended here: no prompt may be shown, and this process can't act as an administrator without one.";

/// Whether this process can elevate unattended: it is an administrator (root, an elevated
/// Windows shell), or on Linux sudo runs a command without a password (`sudo -n true`, through
/// the platform's process runner, which a test scripts). Run before a setup starts, so it fails before
/// anything changed rather than at its first root step.
///
/// # Errors
///
/// Why it can't, with what to do.
pub async fn check_unattended() -> Result<()> {
    if is_root() {
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        if crate::platform::exec::run("sudo", &["-n", "true"], None).await.is_ok() {
            return Ok(());
        }
        return Err(Error::Refused(format!("{CANT_ELEVATE} Run it as root, or where sudo needs no password, or on a terminal without --yes to be asked.")));
    }
    let how = if cfg!(windows) { "Run it from an elevated (administrator) terminal" } else { "Run it as root (sudo)" };
    Err(Error::Refused(format!("{CANT_ELEVATE} {how}, or on a terminal without --yes to be asked.")))
}

#[cfg(test)]
mod tests {
    use super::as_root_with;

    #[test]
    fn a_root_step_prompts_unless_unattended_where_sudo_never_asks() {
        let args = ["sh".to_owned(), "-c".to_owned(), "apt-get install -y x".to_owned()];
        let words = |(p, a): (String, Vec<String>)| std::iter::once(p).chain(a).collect::<Vec<_>>().join(" ");
        assert_eq!(words(as_root_with("env", &args, false, false)), "pkexec env sh -c apt-get install -y x");
        assert_eq!(words(as_root_with("env", &args, false, true)), "pkexec env sh -c apt-get install -y x", "the app prompts whoever runs it");
        assert_eq!(words(as_root_with("env", &args, true, false)), "sudo -n env sh -c apt-get install -y x");
        assert_eq!(words(as_root_with("env", &args, true, true)), "env sh -c apt-get install -y x");
    }
}
