//! The registry: reading values and keys, and the current user's own values.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RRF_RT_ANY, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyValueW, RegGetValueW, RegOpenKeyExW,
    RegSetKeyValueW,
};

use super::wide;

/// A registry hive the launcher reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

impl Hive {
    fn key(self) -> HKEY {
        match self {
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => HKEY_CURRENT_USER,
        }
    }
}

/// The string value `name` under `key`, its %VARIABLES% expanded (`RegGetValueW` expands a
/// `REG_EXPAND_SZ` when asked for `RRF_RT_REG_SZ`). None if absent or unreadable.
pub fn registry_string(hive: Hive, key: &str, name: &str) -> Option<OsString> {
    let (key, name) = (wide(OsStr::new(key)), wide(OsStr::new(name)));
    let mut buf: Vec<u16> = vec![0; 4096];
    // An expanded value can outgrow the size first reported: grow and retry a few times.
    for _ in 0..4 {
        let mut bytes = u32::try_from(buf.len() * 2).ok()?;
        // SAFETY: valid NUL-terminated key/value names; `buf` holds `bytes` bytes.
        let status =
            unsafe { RegGetValueW(hive.key(), key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &raw mut bytes) };
        let units = usize::try_from(bytes).ok()? / 2;
        match status {
            ERROR_SUCCESS => {
                let units = units.min(buf.len());
                let len = buf[..units].iter().position(|&c| c == 0).unwrap_or(units);
                return Some(OsString::from_wide(&buf[..len]));
            }
            ERROR_MORE_DATA => buf.resize(units.max(buf.len() * 2) + 1, 0),
            _ => return None,
        }
    }
    None
}

/// Whether a registry key exists. A key this account may not read still exists.
pub fn registry_key_exists(hive: Hive, path: &str) -> Option<bool> {
    let path = wide(OsStr::new(path));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated key name; `key` receives the handle, closed right after.
    let status = unsafe { RegOpenKeyExW(hive.key(), path.as_ptr(), 0, KEY_READ, &raw mut key) };
    match status {
        ERROR_SUCCESS => {
            // SAFETY: the handle RegOpenKeyExW just opened.
            unsafe { RegCloseKey(key) };
            Some(true)
        }
        ERROR_ACCESS_DENIED => Some(true),
        ERROR_FILE_NOT_FOUND => Some(false),
        _ => None,
    }
}

/// Whether a registry value exists (its data isn't read).
pub fn registry_value_exists(hive: Hive, path: &str, name: &str) -> Option<bool> {
    let (path, name) = (wide(OsStr::new(path)), wide(OsStr::new(name)));
    // SAFETY: NUL-terminated names; no type or data asked for (all three pointers null).
    let status =
        unsafe { RegGetValueW(hive.key(), path.as_ptr(), name.as_ptr(), RRF_RT_ANY, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
    match status {
        ERROR_SUCCESS | ERROR_MORE_DATA => Some(true),
        ERROR_FILE_NOT_FOUND => Some(false),
        _ => None,
    }
}

/// Sets the string value `name` under the current user's `key` to `value`; whether it was.
pub fn set_user_registry_string(key: &str, name: &str, value: &str) -> bool {
    let (key, name, data) = (wide(OsStr::new(key)), wide(OsStr::new(name)), wide(OsStr::new(value)));
    let Ok(bytes) = u32::try_from(data.len() * 2) else { return false };
    // SAFETY: NUL-terminated names; `data` is a NUL-terminated REG_SZ of `bytes` bytes.
    let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), bytes) };
    status == ERROR_SUCCESS
}

/// Deletes the value `name` under the current user's `key`, if it is there.
pub fn delete_user_registry_value(key: &str, name: &str) {
    let (key, name) = (wide(OsStr::new(key)), wide(OsStr::new(name)));
    // SAFETY: NUL-terminated names.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
}
