//! The registry: reading values and keys, and the current user's own values.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_SZ, RRF_RT_ANY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyValueW,
    RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
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

/// A registry value as the launcher reads it: a `REG_DWORD`'s number, or the type of any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryValue {
    Dword(u32),
    Other(u32),
}

/// Every value under `path` (its name and data), None when the key doesn't exist.
///
/// # Errors
///
/// When the key is there but can't be opened or listed (access denied).
pub fn registry_values(hive: Hive, path: &str) -> std::io::Result<Option<Vec<(String, RegistryValue)>>> {
    let path = wide(OsStr::new(path));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated key name; `key` receives the handle, closed below.
    let status = unsafe { RegOpenKeyExW(hive.key(), path.as_ptr(), 0, KEY_READ, &raw mut key) };
    match status {
        ERROR_SUCCESS => {}
        ERROR_FILE_NOT_FOUND => return Ok(None),
        other => return Err(os_error(other)),
    }
    let values = values_of(key);
    // SAFETY: the handle RegOpenKeyExW just opened.
    unsafe { RegCloseKey(key) };
    values.map(Some)
}

/// The values of the open key `key`: each name and type, then a `REG_DWORD`'s data.
fn values_of(key: HKEY) -> std::io::Result<Vec<(String, RegistryValue)>> {
    let mut values = Vec::new();
    // A value name is at most 16,383 characters.
    let mut name = vec![0u16; 16_384];
    for index in 0.. {
        let mut len = u32::try_from(name.len()).map_err(std::io::Error::other)?;
        let mut kind = 0u32;
        // SAFETY: `name` holds `len` characters; the data isn't asked for (null pointers).
        let status =
            unsafe { RegEnumValueW(key, index, name.as_mut_ptr(), &raw mut len, std::ptr::null(), &raw mut kind, std::ptr::null_mut(), std::ptr::null_mut()) };
        match status {
            ERROR_SUCCESS => {}
            ERROR_NO_MORE_ITEMS => break,
            other => return Err(os_error(other)),
        }
        let units = usize::try_from(len).map_err(std::io::Error::other)?.min(name.len());
        let value = if kind == REG_DWORD { dword(key, &name[..units])? } else { RegistryValue::Other(kind) };
        values.push((String::from_utf16_lossy(&name[..units]), value));
    }
    Ok(values)
}

/// The `REG_DWORD` value named `name` (without its NUL) under the open key `key`.
fn dword(key: HKEY, name: &[u16]) -> std::io::Result<RegistryValue> {
    let name: Vec<u16> = name.iter().copied().chain(std::iter::once(0)).collect();
    let mut data = 0u32;
    let mut bytes = 4u32;
    // SAFETY: a NUL-terminated value name; `data` holds the 4 bytes `bytes` says.
    let status = unsafe { RegGetValueW(key, std::ptr::null(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&raw mut data).cast(), &raw mut bytes) };
    if status == ERROR_SUCCESS { Ok(RegistryValue::Dword(data)) } else { Err(os_error(status)) }
}

/// A Win32 error code as an I/O error.
fn os_error(code: u32) -> std::io::Error {
    std::io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
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
