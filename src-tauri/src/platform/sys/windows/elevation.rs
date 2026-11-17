//! Elevation: whether this process runs elevated.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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
