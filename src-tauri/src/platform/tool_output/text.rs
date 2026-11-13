//! A tool's output bytes as text, and the locale it is run in so that text is in the C locale's
//! words.
//!
//! On Windows a console tool writes to a pipe in the system's ANSI or OEM code page unless it
//! writes UTF-8: `VBoxManage list vms` prints a VM named `lab · web` with `·` as the single byte
//! 0xB7 in Windows-1252, which a UTF-8 reading turned into U+FFFD, so the lab's VM was never
//! found. Output that isn't UTF-8 is decoded from the ANSI code page there (then the OEM one).
//!
//! On Linux and macOS each tool's messages, numbers and times are in the C locale, its character
//! set left as the user's (`LC_CTYPE`): a user's `LC_ALL` moves to `LC_CTYPE`, `LANGUAGE` is
//! dropped.

use std::ffi::OsString;

/// The locale categories a tool runs with in the C locale.
const C_CATEGORIES: [&str; 3] = ["LC_MESSAGES", "LC_NUMERIC", "LC_TIME"];

/// What changes in a tool's environment, given the launcher's own variables (`var`): each
/// variable set to its value, or removed (`None`). Nothing on Windows, whose tools don't read
/// them.
pub fn locale(var: impl Fn(&str) -> Option<OsString>) -> Vec<(&'static str, Option<OsString>)> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut out: Vec<(&'static str, Option<OsString>)> = Vec::new();
    if let Some(all) = var("LC_ALL").filter(|v| !v.is_empty()) {
        out.push(("LC_CTYPE", Some(all)));
    }
    out.push(("LC_ALL", None));
    out.push(("LANGUAGE", None));
    out.extend(C_CATEGORIES.map(|c| (c, Some(OsString::from("C")))));
    out
}

/// `bytes`, a tool's output, as text: UTF-8 when it is; else, on Windows, decoded from the
/// system's ANSI code page (then its OEM one); else read as UTF-8 with what isn't replaced.
#[must_use]
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => windows_code_page(bytes).unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// `bytes` decoded from the ANSI code page, else the OEM one; none off Windows or when neither
/// reads them.
#[cfg(windows)]
fn windows_code_page(bytes: &[u8]) -> Option<String> {
    use windows_sys::Win32::Globalization::{CP_ACP, CP_OEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar};
    let len = i32::try_from(bytes.len()).ok()?;
    [CP_ACP, CP_OEMCP].into_iter().find_map(|page| {
        // SAFETY: `bytes` is valid for `len` bytes; the first call asks for the length only (no
        // output buffer), the second writes at most `wide.len()` units into `wide`.
        let units = unsafe { MultiByteToWideChar(page, MB_ERR_INVALID_CHARS, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
        let mut wide = vec![0u16; usize::try_from(units).ok().filter(|n| *n > 0)?];
        // SAFETY: as above, `wide` holds `units` units.
        let written = unsafe { MultiByteToWideChar(page, MB_ERR_INVALID_CHARS, bytes.as_ptr(), len, wide.as_mut_ptr(), units) };
        wide.truncate(usize::try_from(written).ok()?);
        String::from_utf16(&wide).ok()
    })
}

/// No code page off Windows: tools write UTF-8 there.
#[cfg(not(windows))]
fn windows_code_page(_bytes: &[u8]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_output_reads_as_it_is() {
        assert_eq!(decode("\"lab · web\" {6f2c}\n".as_bytes()), "\"lab · web\" {6f2c}\n");
        assert_eq!(decode(b""), "");
    }

    /// Regression (#371): `VBoxManage`'s list in Windows-1252, its `·` one byte (0xB7).
    #[test]
    fn a_windows_code_page_listing_keeps_its_middle_dot_on_windows() {
        let ansi = include_bytes!("fixtures/vboxmanage/7.1.4-windows-1252-list-vms.txt");
        let read = decode(ansi);
        if cfg!(windows) {
            assert!(read.starts_with("\"lab · web\" {"), "{read}");
        } else {
            assert!(read.starts_with("\"lab \u{fffd} web\" {"), "UTF-8 only off Windows: {read}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_french_session_keeps_its_character_set_and_hears_tools_in_c() {
        let of = |vars: &[(&str, &str)]| {
            locale(|n| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| OsString::from(v)))
                .into_iter()
                .map(|(k, v)| (k, v.map(|v| v.to_string_lossy().into_owned())))
                .collect::<Vec<_>>()
        };
        let c = Some("C".to_owned());
        assert_eq!(
            of(&[("LANG", "fr_FR.UTF-8")]),
            [("LC_ALL", None), ("LANGUAGE", None), ("LC_MESSAGES", c.clone()), ("LC_NUMERIC", c.clone()), ("LC_TIME", c)]
        );
        assert_eq!(of(&[("LC_ALL", "de_DE.UTF-8")]).first(), Some(&("LC_CTYPE", Some("de_DE.UTF-8".to_owned()))));
    }
}
