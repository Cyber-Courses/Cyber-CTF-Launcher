//! Tools that may ship as a `.cmd`/`.bat` wrapper on Windows (not a real `.exe`). `CreateProcessW`
//! only appends `.exe` and does not consult `PATHEXT`, so `Command::new("az")` can't find
//! `az.cmd`; these go through `cmd.exe` (which also resolves a plain `.exe`, so it's safe for a
//! tool whose Windows form varies, like `oci`). `aws`/`terraform`/`docker`/`vagrant` are real
//! `.exe` and resolve directly.
//!
//! cmd.exe parses the rest of its command line, so an argument holding one of its operators
//! could run another command. Every value passed to these tools is validated where it comes from
//! (`runtime/server/input.rs`), and [`check_args`] refuses any that isn't, on every OS, so a new
//! call with an unchecked value fails in the tests instead of becoming an injection on Windows.

use crate::error::{Error, Result};

/// The tools run through `cmd /C` on Windows.
pub(super) const SHIMMED: &[&str] = &["az", "gcloud", "oci"];

/// What cmd.exe gives a meaning to outside quotes: command separators and pipes, redirections,
/// its escape character, variable expansion, quotes, and line breaks.
const CMD_OPERATORS: &[char] = &['&', '|', '<', '>', '^', '%', '"', '\n', '\r'];

/// Refuses an argument cmd.exe would interpret, for a tool that goes through it.
pub(super) fn check_args(program: &str, args: &[&str]) -> Result<()> {
    if !SHIMMED.contains(&program) {
        return Ok(());
    }
    match args.iter().find(|a| a.contains(CMD_OPERATORS)) {
        Some(arg) => Err(Error::Invalid(format!("refusing to pass `{}` to {program}: it holds a character cmd.exe would interpret", arg.escape_debug()))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::check_args;

    #[test]
    fn the_arguments_the_launcher_passes_go_through() {
        assert!(check_args("gcloud", &["auth", "list", "--filter=status:ACTIVE", "--format=value(account)"]).is_ok());
        assert!(check_args("az", &["account", "list", "--query", "[].{name:name,id:id,isDefault:isDefault}", "--output", "json"]).is_ok());
        assert!(check_args("oci", &["iam", "region", "list", "--profile", "DEFAULT"]).is_ok());
    }

    #[test]
    fn a_cmd_operator_in_an_argument_is_refused() {
        for bad in ["x & calc", "a|b", "out>f", "<in", "^x", "%PATH%", "say \"hi\"", "a\nb", "a\rb"] {
            let err = check_args("az", &["group", "create", "--name", bad]).unwrap_err().to_string();
            assert!(err.contains("cmd.exe"), "{bad}: {err}");
        }
    }

    #[test]
    fn tools_that_run_directly_are_not_restricted() {
        assert!(check_args("aws", &["s3", "ls", "a & b"]).is_ok());
        assert!(check_args("docker", &["run", "--entrypoint", "sh", "img", "-c", "a | b"]).is_ok());
    }

    #[tokio::test]
    async fn a_refused_argument_never_reaches_the_tool() {
        // No fake rule matches: had the guard not refused it, this would try to spawn `az`.
        let err = crate::exec::run("az", &["group", "create", "--name", "x&calc"], None).await.unwrap_err();
        assert!(err.to_string().contains("cmd.exe"), "{err}");
        let lines = std::cell::RefCell::new(Vec::new());
        let err = crate::exec::stream("gcloud", &["projects", "create", "a|b"], None, &[], |l| lines.borrow_mut().push(l)).await.unwrap_err();
        assert!(err.to_string().contains("cmd.exe"), "{err}");
        assert!(lines.borrow().is_empty());
    }
}
