//! Elevation: whether this process may manage Hyper-V, and starting a program elevated through
//! a UAC prompt (`ShellExecuteEx` with the `runas` verb).

use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, CheckTokenMembership, FreeSid, GetTokenInformation, PSID, SID_IDENTIFIER_AUTHORITY, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

use super::wide;

/// `SW_HIDE`: the program keeps its console window hidden.
const SW_HIDE: i32 = 0;

/// Whether this process runs with an administrator (elevated) token.
pub fn is_elevated() -> bool {
    let Ok(size) = u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()) else { return false };
    // SAFETY: plain Win32 calls on a token handle owned (and closed) here, filling a struct of
    // the size given.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, std::ptr::from_mut(&mut elevation).cast(), size, &raw mut len) != 0;
        CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Whether this process's token is in BUILTIN\Hyper-V Administrators (S-1-5-32-578).
pub fn in_hyperv_admins() -> bool {
    const SECURITY_NT_AUTHORITY: SID_IDENTIFIER_AUTHORITY = SID_IDENTIFIER_AUTHORITY { Value: [0, 0, 0, 0, 0, 5] };
    const SECURITY_BUILTIN_DOMAIN_RID: u32 = 32;
    const DOMAIN_ALIAS_RID_HYPER_V_ADMINS: u32 = 578;
    // SAFETY: the SID is allocated, checked against this thread's (or process's) token, and
    // freed here.
    unsafe {
        let mut sid: PSID = std::ptr::null_mut();
        if AllocateAndInitializeSid(&SECURITY_NT_AUTHORITY, 2, SECURITY_BUILTIN_DOMAIN_RID, DOMAIN_ALIAS_RID_HYPER_V_ADMINS, 0, 0, 0, 0, 0, 0, &raw mut sid)
            == 0
        {
            return false;
        }
        let mut member = 0;
        let ok = CheckTokenMembership(std::ptr::null_mut(), sid, &raw mut member) != 0;
        FreeSid(sid);
        ok && member != 0
    }
}

/// A program started elevated; its handle is closed when dropped.
pub struct ElevatedProcess(HANDLE);

// SAFETY: a process handle is a kernel handle, usable from any thread.
unsafe impl Send for ElevatedProcess {}

impl ElevatedProcess {
    /// Waits for it to end; its exit code.
    pub fn wait(self) -> u32 {
        let mut code = 1u32;
        // SAFETY: the live process handle ShellExecuteExW returned (closed on drop).
        unsafe {
            WaitForSingleObject(self.0, INFINITE);
            if GetExitCodeProcess(self.0, &raw mut code) == 0 {
                code = 1;
            }
        }
        code
    }
}

impl Drop for ElevatedProcess {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Starts `exe` elevated with `args`, in `dir` (UAC asks first, for the window `owner` when not
/// 0). The OS error code when it couldn't (1223: the prompt was declined).
pub fn launch_elevated(exe: &Path, args: &str, dir: &Path, owner: isize) -> Result<ElevatedProcess, i32> {
    let (verb, file, params, directory) = (wide("runas".as_ref()), wide(exe.as_os_str()), wide(args.as_ref()), wide(dir.as_os_str()));
    let Ok(size) = u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()) else { return Err(1) };
    // SAFETY: a zeroed SHELLEXECUTEINFOW is valid (null pointers, no flags); every string it
    // points to outlives the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.hwnd = owner as _;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.lpDirectory = directory.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&raw mut info) == 0 {
            return Err(GetLastError().cast_signed());
        }
        if info.hProcess.is_null() {
            return Err(1);
        }
        Ok(ElevatedProcess(info.hProcess))
    }
}
