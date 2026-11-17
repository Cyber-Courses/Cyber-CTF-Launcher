//! Joining "Hyper-V Administrators" through one UAC prompt (#185), and the window a UAC prompt is
//! raised for. Hyper-V management needs an administrator (or a member of that group); a lab's
//! Vagrant and the VM self-test run inside the Isothrum runner, which elevates on its own.

use crate::error::{Error, Result};

/// The longest command line Windows accepts, in UTF-16 units.
#[cfg(any(windows, test))]
const MAX_COMMAND_LINE: usize = 32_000;

/// The arguments of `powershell.exe` that run `script`.
#[cfg(any(windows, test))]
fn powershell_args(script: &str) -> Result<Vec<String>> {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let args: Vec<String> = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-EncodedCommand"]
        .into_iter()
        .map(String::from)
        .chain([base64::engine::general_purpose::STANDARD.encode(utf16)])
        .collect();
    if args.iter().map(|a| a.len() + 1).sum::<usize>() > MAX_COMMAND_LINE {
        return Err(Error::Invalid("the script is too long for a Windows command line".into()));
    }
    Ok(args)
}

/// The well-known SID of BUILTIN\Hyper-V Administrators (its name is translated).
#[cfg(any(windows, test))]
const HYPERV_ADMINS_SID: &str = "S-1-5-32-578";

/// The PowerShell script adding `account` (`DOMAIN\user`) to Hyper-V Administrators; one already
/// in it is fine. Any other failure ends it with a non-zero exit code.
#[cfg(any(windows, test))]
fn join_hyperv_admins_script(account: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\ntry {{ Add-LocalGroupMember -SID {} -Member {} }} catch [Microsoft.PowerShell.Commands.MemberExistsException] {{ }}\n",
        ps_quote(HYPERV_ADMINS_SID),
        ps_quote(account)
    )
}

/// What declining the prompt (or another refusal) to join Hyper-V Administrators means.
#[cfg(any(windows, test))]
fn join_error(code: i32) -> Error {
    const ERROR_CANCELLED: i32 = 1223;
    match code {
        ERROR_CANCELLED => Error::Invalid("The Windows permission prompt was declined, so nothing was changed. Try again and choose Yes.".into()),
        _ => Error::Invalid(format!("Windows couldn't add your account to Hyper-V Administrators ({}).", std::io::Error::from_raw_os_error(code))),
    }
}

/// Runs PowerShell `script` elevated, through one UAC prompt, and waits for its exit code. `Err`
/// carries the Windows error that kept it from starting (`ERROR_CANCELLED`: the prompt was
/// declined).
#[cfg(windows)]
async fn elevated_powershell(script: &str) -> Result<std::result::Result<u32, i32>> {
    let args = powershell_args(script)?.join(" ");
    let exe = powershell();
    let dir = std::env::temp_dir();
    let owner = owner_window().unwrap_or(0);
    tokio::task::spawn_blocking(move || crate::platform::sys::launch_elevated(&exe, &args, &dir, owner).map(crate::platform::sys::ElevatedProcess::wait))
        .await
        .map_err(|e| Error::Invalid(e.to_string()))
}

/// Adds this user to "Hyper-V Administrators", through one UAC prompt (#185). From the next
/// sign-in on, Vagrant drives Hyper-V for this account without administrator rights, status
/// reads included, so the app needn't run as administrator nor ask again for each lab.
#[cfg(windows)]
pub async fn join_hyperv_admins() -> Result<()> {
    let account = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(user)) => format!("{domain}\\{user}"),
        (_, Ok(user)) => user,
        _ => return Err(Error::Invalid("couldn't tell which Windows account this is".into())),
    };
    let code = elevated_powershell(&join_hyperv_admins_script(&account)).await?.map_err(join_error)?;
    if code != 0 {
        return Err(Error::Invalid(format!("Windows couldn't add your account to Hyper-V Administrators (PowerShell exited with {code}).")));
    }
    Ok(())
}

/// Hyper-V is Windows-only: refused elsewhere.
#[cfg(not(windows))]
pub async fn join_hyperv_admins() -> Result<()> {
    Err(Error::Invalid("Hyper-V is only on Windows.".into()))
}

/// The environment variable a deploy worker gets the app's main window in (see
/// [`set_owner_window`]).
#[cfg(any(windows, test))]
pub const OWNER_WINDOW_ENV: &str = "CYBERCTF_OWNER_WINDOW";

#[cfg(any(windows, test))]
static OWNER_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// The app's main window (a Windows `HWND`): the UAC prompt is raised for it, so it comes to the
/// front instead of waiting as a flashing taskbar button behind a windowless process.
#[cfg(any(windows, test))]
pub fn set_owner_window(hwnd: isize) {
    OWNER_WINDOW.store(hwnd, std::sync::atomic::Ordering::Relaxed);
}

/// The window UAC prompts are raised for: the one set here, else the one a deploy worker was
/// started with.
#[cfg(any(windows, test))]
pub fn owner_window() -> Option<isize> {
    let set = OWNER_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    if set != 0 {
        return Some(set);
    }
    std::env::var(OWNER_WINDOW_ENV).ok().and_then(|v| v.parse().ok()).filter(|h| *h != 0)
}

/// Windows PowerShell, by its full path.
#[cfg(windows)]
fn powershell() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe")
}

/// `s` as a PowerShell single-quoted string. PowerShell also takes the typographic single quotes
/// as quote characters, so every one of them is doubled too.
#[cfg(any(windows, test))]
fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joining_hyperv_administrators_adds_this_account_by_the_groups_sid() {
        // #185: the group's name is translated (Administrateurs Hyper-V), its SID isn't.
        let script = join_hyperv_admins_script(r"PC\Jo O'Brien");
        assert!(script.contains(r"Add-LocalGroupMember -SID 'S-1-5-32-578' -Member 'PC\Jo O''Brien'"), "{script}");
        assert!(script.contains("MemberExistsException") && script.starts_with("$ErrorActionPreference = 'Stop'"), "{script}");
        assert!(join_error(1223).to_string().contains("declined"));
        assert!(join_error(5).to_string().contains("Hyper-V Administrators"));
    }

    #[test]
    fn the_owner_window_comes_from_the_app_or_the_workers_environment() {
        // No window set in this test process, nor in its environment.
        assert_eq!(owner_window(), None);
        set_owner_window(42);
        assert_eq!(owner_window(), Some(42));
        set_owner_window(0);
    }

    #[test]
    fn a_script_too_long_for_a_command_line_is_refused() {
        assert!(powershell_args("Write-Output 'x'").unwrap().contains(&"-EncodedCommand".to_owned()));
        assert!(powershell_args(&"x".repeat(MAX_COMMAND_LINE)).is_err());
    }
}
