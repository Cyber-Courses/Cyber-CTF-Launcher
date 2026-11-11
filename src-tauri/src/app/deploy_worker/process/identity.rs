//! A process's identity the clock can't move: the boot it runs in and when it started within it,
//! both as the kernel stored them, never computed from the wall clock.
//!
//! A worker was recognised by its start time in seconds since the epoch, which on Linux is
//! computed (the boot time, itself now minus the uptime, plus the start in ticks since boot): a
//! clock set an hour forward moved it an hour, and a live worker read as another process, its
//! pidfile swept and its job settled as lost. What this reads instead:
//!
//! - Linux: `/proc/<pid>/stat`'s start time, in clock ticks since boot, and the boot's id
//!   (`/proc/sys/kernel/random/boot_id`);
//! - macOS: the start time the kernel stored at the fork (`proc_bsdinfo`, as `kinfo_proc`'s
//!   `p_starttime`) and the boot's id (`kern.bootsessionuuid`: `kern.boottime` is shifted when
//!   the clock is set);
//! - Windows: the creation time the kernel stored, and the boot's number (`BootId`).

use std::fmt;

/// Where a process stands for good: its boot and its start within that boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The boot, as a number (a boot id's 128 bits, or Windows' boot count).
    pub boot: u128,
    /// Its start: ticks since boot (Linux), microseconds since the epoch as stored at its fork
    /// (macOS), 100 ns since 1601 as stored at its creation (Windows).
    pub start: u64,
}

impl Identity {
    /// The pidfile's word for it ([`fmt::Display`]): `<boot, hex>-<start>`.
    pub fn parse(word: &str) -> Option<Identity> {
        let (boot, start) = word.split_once('-')?;
        Some(Identity { boot: u128::from_str_radix(boot, 16).ok()?, start: start.parse().ok()? })
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x}-{}", self.boot, self.start)
    }
}

/// A boot id as Linux and macOS write it (a UUID, with or without dashes) as a number.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn boot_number(text: &str) -> Option<u128> {
    let hex: String = text.trim().chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

/// The start time in a `/proc/<pid>/stat` line: field 22, counted after the command name, which
/// sits in parentheses and may itself hold spaces and parentheses.
#[cfg(any(target_os = "linux", test))]
fn stat_start(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    // After the name: state (field 3) … starttime (field 22), so the 20th word.
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// The identity of process `pid` from its `stat` line and this boot's id.
#[cfg(any(target_os = "linux", test))]
fn from_proc(stat: &str, boot_id: &str) -> Option<Identity> {
    Some(Identity { boot: boot_number(boot_id)?, start: stat_start(stat)? })
}

/// The identity of process `pid`, when the OS gives it.
#[cfg(target_os = "linux")]
pub fn of(pid: u32) -> Option<Identity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    from_proc(&stat, &boot)
}

/// [`of`] on macOS.
#[cfg(target_os = "macos")]
pub fn of(pid: u32) -> Option<Identity> {
    let (secs, micros) = crate::platform::sys::process_start(pid)?;
    let boot = boot_number(&crate::platform::sys::boot_session()?)?;
    Some(Identity { boot, start: secs.checked_mul(1_000_000)?.checked_add(micros)? })
}

/// [`of`] on Windows. Without the boot's number (a policy hiding it), the creation time alone,
/// stored in 100 ns ticks, still tells one process from another.
#[cfg(windows)]
pub fn of(pid: u32) -> Option<Identity> {
    let start = crate::platform::sys::process_created(pid)?;
    Some(Identity { boot: windows_boot().unwrap_or(0), start })
}

/// This boot's number on Windows: the count of boots the kernel keeps.
#[cfg(windows)]
fn windows_boot() -> Option<u128> {
    use crate::platform::sys::{Hive, RegistryValue, registry_values};
    const KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters";
    let values = registry_values(Hive::LocalMachine, KEY).ok()??;
    values.into_iter().find_map(|(name, value)| match value {
        RegistryValue::Dword(n) if name.eq_ignore_ascii_case("BootId") => Some(u128::from(n)),
        _ => None,
    })
}

/// Elsewhere none: the start time alone recognises a worker.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn of(_pid: u32) -> Option<Identity> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `/proc/<pid>/stat` line (from a real one), the command name holding spaces and a `)`.
    const STAT: &str = "4242 (sh) deploy (x)) S 1 4242 4242 0 -1 4194560 120 0 0 0 0 0 0 0 20 0 1 0 987654 2240512 210 18446744073709551615 1 1 0 0 0 0 0 0 65538 0 0 0 17 3 0 0 0 0 0";
    const BOOT: &str = "6f1e2c3a-9b8d-4e7f-a1b2-c3d4e5f60718\n";

    #[test]
    fn a_proc_stat_line_gives_its_start_in_ticks_since_boot() {
        assert_eq!(stat_start(STAT), Some(987_654));
        assert_eq!(stat_start("4242 (sh) S 1"), None, "cut short");
        assert_eq!(stat_start("no name"), None);
        let id = from_proc(STAT, BOOT).unwrap();
        assert_eq!(id, Identity { boot: 0x6f1e_2c3a_9b8d_4e7f_a1b2_c3d4_e5f6_0718, start: 987_654 });
        assert_eq!(from_proc(STAT, "not a boot id"), None);
    }

    #[test]
    fn an_identity_reads_back_from_its_pidfile_word() {
        let id = Identity { boot: boot_number("AF939031-CA2E-46F4-BC26-1086701FC549").unwrap(), start: 1_793_743_213_433_990 };
        assert_eq!(Identity::parse(&id.to_string()), Some(id));
        for bad in ["", "-", "x-1", "1-x", "1"] {
            assert_eq!(Identity::parse(bad), None, "{bad:?}");
        }
    }

    /// The OS's own answer: the same for one process read twice, another for another process.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn a_process_keeps_its_identity_and_another_has_its_own() {
        let me = of(std::process::id()).expect("the OS describes this very process");
        assert_eq!(of(std::process::id()), Some(me));
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd").args(["/C", "ping -n 3 127.0.0.1 >NUL"]).spawn()
        } else {
            std::process::Command::new("sleep").arg("2").spawn()
        }
        .unwrap();
        let other = of(child.id()).expect("a child the OS describes");
        assert_eq!(other.boot, me.boot, "one boot");
        assert_ne!(other, me);
        child.kill().ok();
        child.wait().unwrap();
        // Windows keeps an exited process's record while any handle to it is open: `Child`'s
        // until it drops, and another's for a moment after (a process listing in a test running
        // beside this one, an antivirus). Its identity goes once nothing holds it; the id may name
        // another process by then, never this one.
        let pid = child.id();
        drop(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while of(pid) == Some(other) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_ne!(of(pid), Some(other), "a reaped child is gone");
    }
}
