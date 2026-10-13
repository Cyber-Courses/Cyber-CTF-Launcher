//! What a failed Windows install's exit codes mean, in words: winget's own result codes and the
//! codes an installer it ran exits with (a cancelled UAC prompt, an MSI the player backed out of).
//! The raw codes stay in the setup log and in the error, after the summary.

/// winget's "the installer failed" code: what went wrong is the installer's own exit code,
/// which winget prints ("Installer failed with exit code: …").
const WINGET_INSTALLER_FAILED: u32 = 0x8A15_0006;

/// What winget's own result codes mean (`APPINSTALLER_CLI_ERROR_*`, documented in winget-cli's
/// doc/windows/package-manager/winget/returnCodes.md).
fn winget_summary(code: u32) -> Option<Outcome> {
    Some(match code {
        0x8A15_010C => Outcome::Cancelled,
        0x8A15_0102 => Outcome::Failed("Another installation is already running. Wait for it to finish, then try again."),
        0x8A15_0105 => Outcome::Failed("There isn't enough disk space. Free up some space, then try again."),
        0x8A15_0107 => Outcome::Failed("This needs an internet connection. Check your connection, then try again."),
        0x8A15_0109 => Outcome::Failed("Restart Windows to finish."),
        0x8A15_010A => Outcome::Failed("Restart Windows, then try again."),
        0x8A15_010F => Outcome::Failed("This is blocked by a policy on this computer. Ask your administrator."),
        WINGET_INSTALLER_FAILED => Outcome::Failed("The installer reported an error, so it didn't finish. The details are in the log above."),
        _ => return None,
    })
}

/// Installer exit codes that mean the player said no (to the UAC prompt or the installer).
fn installer_cancelled(code: u32) -> bool {
    matches!(
        code,
        1223 // ERROR_CANCELLED: the UAC prompt was declined
            | 1602 // ERROR_INSTALL_USEREXIT: an MSI cancelled
            | 0x8007_04C7 // ERROR_CANCELLED as an HRESULT
            | 0xFFFF_FFFB // Docker Desktop's installer when its UAC prompt is declined
    )
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Cancelled,
    Failed(&'static str),
}

/// The installer's exit code in a winget output line ("Installer failed with exit code: 1602").
/// Only winget's English output says it this way; in other languages the summary falls back to
/// winget's own code.
pub fn installer_exit_code(line: &str) -> Option<u32> {
    let rest = line.trim().strip_prefix("Installer failed with exit code:")?.trim();
    match rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => rest.parse::<u32>().ok().or_else(|| rest.parse::<i32>().ok().map(|c| c as u32)),
    }
}

/// A human-readable summary of a failed winget run, if its codes are known. `removing` words a
/// cancel for an uninstall.
pub fn winget_failure(code: Option<i32>, installer_code: Option<u32>, removing: bool) -> Option<&'static str> {
    let code = code? as u32;
    let outcome = match installer_code {
        Some(c) if code == WINGET_INSTALLER_FAILED && installer_cancelled(c) => Outcome::Cancelled,
        _ => winget_summary(code)?,
    };
    Some(summary(outcome, removing))
}

/// A summary of a failed installer the app ran itself (WSL's, through PowerShell), from the
/// installer's own exit code: only a cancel is known.
pub fn installer_failure(code: Option<i32>, removing: bool) -> Option<&'static str> {
    installer_cancelled(code? as u32).then(|| summary(Outcome::Cancelled, removing))
}

fn summary(outcome: Outcome, removing: bool) -> &'static str {
    match outcome {
        Outcome::Cancelled if removing => "The removal was cancelled, so nothing was changed. Try again and accept the administrator prompt.",
        Outcome::Cancelled => "The installation was cancelled, so nothing was installed. Try again and accept the administrator prompt.",
        Outcome::Failed(why) => why,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declined_uac_prompt_reads_as_cancelled() {
        // The issue's run: Docker Desktop's installer exits 0xFFFFFFFB, winget then 0x8A150006.
        let installer = installer_exit_code("Installer failed with exit code: 4294967291");
        assert_eq!(installer, Some(0xFFFF_FFFB));
        let summary = winget_failure(Some(0x8A15_0006_u32 as i32), installer, false).unwrap();
        assert!(summary.starts_with("The installation was cancelled"), "{summary}");
        assert!(winget_failure(Some(0x8A15_0006_u32 as i32), installer, true).unwrap().starts_with("The removal was cancelled"));
        // An MSI the player backed out of, and winget's own "cancelled by user".
        assert!(winget_failure(Some(0x8A15_0006_u32 as i32), Some(1602), false).unwrap().contains("cancelled"));
        assert!(winget_failure(Some(0x8A15_010C_u32 as i32), None, false).unwrap().contains("cancelled"));
    }

    #[test]
    fn an_installer_error_reads_as_failed() {
        let failed = |installer| winget_failure(Some(0x8A15_0006_u32 as i32), installer, false).unwrap();
        assert!(failed(Some(1603)).starts_with("The installer reported an error"));
        // Not English output: winget's code alone still says it failed.
        assert!(failed(None).starts_with("The installer reported an error"));
        assert!(winget_failure(Some(0x8A15_0105_u32 as i32), None, false).unwrap().contains("disk space"));
    }

    #[test]
    fn unknown_codes_get_no_summary() {
        assert_eq!(winget_failure(Some(1), None, false), None);
        assert_eq!(winget_failure(None, None, false), None);
        assert_eq!(installer_failure(Some(1), false), None);
        assert_eq!(installer_failure(None, false), None);
    }

    #[test]
    fn an_installer_the_app_ran_itself_reads_a_declined_prompt_as_cancelled() {
        assert!(installer_failure(Some(1223), false).unwrap().starts_with("The installation was cancelled"));
        assert!(installer_failure(Some(1602), true).unwrap().starts_with("The removal was cancelled"));
    }

    #[test]
    fn installer_codes_at_the_edges_of_the_range() {
        let code = |rest: &str| installer_exit_code(&format!("Installer failed with exit code: {rest}"));
        // The sign bit either way, and the top of the unsigned range.
        assert_eq!(code("0x80000000"), Some(0x8000_0000));
        assert_eq!(code("-2147483648"), Some(0x8000_0000));
        assert_eq!(code("4294967295"), Some(u32::MAX));
        assert_eq!(code("0X10"), Some(16));
        // Past either range, an empty hex form, too many hex digits, and trailing text: not a code.
        for bad in ["4294967296", "-2147483649", "0x", "0xFFFFFFFFF", "1602 (cancelled)", "+", ""] {
            assert_eq!(code(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn winget_codes_at_the_edges_map_without_overflow() {
        // The process exit code comes as i32: the sign bit set is still winget's HRESULT space.
        assert_eq!(winget_failure(Some(i32::MIN), None, false), None);
        assert_eq!(winget_failure(Some(-1), None, false), None);
        assert!(installer_failure(Some(-5), true).unwrap().starts_with("The removal was cancelled"));
        // winget's own "cancelled" wins whatever the installer said; an installer cancel only
        // counts under winget's "installer failed" code, and 0 is never a cancel.
        assert!(winget_failure(Some(0x8A15_010C_u32 as i32), Some(1603), false).unwrap().contains("cancelled"));
        assert!(winget_failure(Some(0x8A15_0105_u32 as i32), Some(1602), false).unwrap().contains("disk space"));
        assert!(winget_failure(Some(0x8A15_0006_u32 as i32), Some(0), false).unwrap().starts_with("The installer reported an error"));
    }

    #[test]
    fn installer_codes_parse_in_each_form() {
        assert_eq!(installer_exit_code("Installer failed with exit code: 1602"), Some(1602));
        assert_eq!(installer_exit_code("  Installer failed with exit code: -5 "), Some(0xFFFF_FFFB));
        assert_eq!(installer_exit_code("Installer failed with exit code: 0x80070643"), Some(0x8007_0643));
        assert_eq!(installer_exit_code("Starting package install..."), None);
        assert_eq!(installer_exit_code("Installer failed with exit code: soon"), None);
    }
}

#[cfg(test)]
mod proptests;
