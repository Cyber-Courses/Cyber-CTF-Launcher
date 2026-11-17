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

/// cmd.exe's exit code when it finds no such command ("'oci' is not recognized as an internal
/// or external command, operable program or batch file.").
const CMD_NOT_FOUND: i32 = 9009;

/// Whether `program`, run through cmd.exe, isn't installed: cmd itself started (so spawning it
/// succeeded) but exited with its own "not found" code. No tool exits with it elsewhere (unix
/// exit codes stop at 255).
pub(super) fn missing_behind_cmd(program: &str, code: Option<i32>) -> bool {
    SHIMMED.contains(&program) && code == Some(CMD_NOT_FOUND)
}

/// Refuses an argument cmd.exe would interpret, for a tool that goes through it.
pub(crate) fn check_args(program: &str, args: &[&str]) -> Result<()> {
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
        // The Machine page's Azure CLI probe (a quoted `--query` here read as "not installed").
        assert!(check_args("az", &["version", "--output", "json"]).is_ok());
    }

    #[test]
    fn a_cmd_operator_in_an_argument_is_refused() {
        for bad in ["x & calc", "a|b", "out>f", "<in", "^x", "%PATH%", "say \"hi\"", "a\nb", "a\rb"] {
            let err = check_args("az", &["group", "create", "--name", bad]).unwrap_err().to_string();
            assert!(err.contains("cmd.exe"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_wrapped_tool_cmd_cant_find_is_missing() {
        // Regression: `cmd /C oci` with no OCI CLI exits 9009, which read as "OCI rejected the
        // request" instead of "install the OCI CLI".
        use super::missing_behind_cmd;
        for tool in ["az", "gcloud", "oci"] {
            assert!(missing_behind_cmd(tool, Some(9009)), "{tool}");
            assert!(!missing_behind_cmd(tool, Some(1)) && !missing_behind_cmd(tool, None), "{tool}");
        }
        // A tool that runs directly reports a missing binary at spawn time instead.
        assert!(!missing_behind_cmd("terraform", Some(9009)));
        let err = crate::platform::exec::exit_error("oci", &["iam", "region", "list"], Some(9009), "'oci' is not recognized".into());
        assert!(matches!(err, crate::error::Error::ToolMissing { tool: "oci" }), "{err:?}");
        let err = crate::platform::exec::exit_error("oci", &["iam"], Some(1), "ServiceError".into());
        assert!(matches!(err, crate::error::Error::CommandFailed { .. }), "{err:?}");
    }

    #[test]
    fn tools_that_run_directly_are_not_restricted() {
        assert!(check_args("aws", &["s3", "ls", "a & b"]).is_ok());
        assert!(check_args("docker", &["run", "--entrypoint", "sh", "img", "-c", "a | b"]).is_ok());
    }

    #[test]
    fn values_cmd_treats_as_plain_text_go_through_and_a_refusal_shows_the_argument_escaped() {
        // An empty value, a tab, parentheses outside a block, `=`/`;`/`,` (batch delimiters that
        // `%*` passes along whole) and non-ASCII are not operators: a size or a profile name with
        // them reaches the tool as typed, since nothing is run through a shell but cmd.exe itself.
        assert!(check_args("az", &["", "a\tb", "(x)", "a=b;c,d", "caf\u{e9}", "--size", "Standard D2"]).is_ok());
        // The refusal quotes the value with its control characters visible, so a log says which.
        let err = check_args("gcloud", &["projects", "create", "a\nb"]).unwrap_err().to_string();
        assert!(err.contains("`a\\nb`") && err.contains("gcloud"), "{err}");
    }

    #[test]
    fn a_quoted_query_is_refused() {
        // Regression (#219): the Machine page's Azure probe passed `"azure-cli"` (quotes included)
        // to az; the guard refused it on every OS, and az read as not installed. isothrum-machine
        // asks the tools now: the launcher passes none of them a literal argument.
        assert!(check_args("az", &["version", "--query", "\"azure-cli\""]).is_err());
    }

    #[tokio::test]
    async fn a_refused_argument_never_reaches_the_tool() {
        // No fake rule matches: had the guard not refused it, this would try to spawn `az`.
        let err = crate::platform::exec::run("az", &["group", "create", "--name", "x&calc"], None).await.unwrap_err();
        assert!(err.to_string().contains("cmd.exe"), "{err}");
    }
}
