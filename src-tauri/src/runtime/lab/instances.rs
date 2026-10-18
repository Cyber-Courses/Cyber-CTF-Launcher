//! The lab's Isoloom instance number: picked once per lab, kept in a marker file, and chosen
//! so its Docker blocks clear every other running lab's.

use std::path::Path;

use super::{Lab, lab};
use crate::error::{Error, Result};

/// The lab's Isoloom instance number (see the module), when it has one.
pub const INSTANCE_MARKER: &str = ".cyberctf-instance";

pub fn instance(dir: &Path) -> Option<u8> {
    std::fs::read_to_string(dir.join(INSTANCE_MARKER)).ok()?.trim().parse().ok().filter(|n| (1..=isoloom_spec::instance::MAX).contains(n))
}

/// Gives the lab an instance number if it has none: the lowest one no other installed lab
/// under `labs` holds, whose Docker blocks overlap none of `in_use` (the subnets of the Docker
/// networks already on this machine). An instance moves the spec's blocks by its number, so two labs with
/// different numbers can still land on one subnet; Docker would refuse the second. Returns
/// the lab's number.
pub fn ensure_instance(labs: &Path, dir: &Path, in_use: &[String]) -> Result<u8> {
    let _picking = lock_instances(labs)?;
    match instance(dir) {
        Some(n) => Ok(n),
        None => pick_instance(labs, dir, in_use),
    }
}

/// [`ensure_instance`] at a start, for a lab that isn't running: its number stays unless its
/// blocks overlap one of `in_use` now (picked while Docker was down and couldn't say what it
/// had, or taken since), in which case it moves to a free one. Docker would otherwise refuse
/// its networks ("Pool overlaps").
pub fn recheck_instance(labs: &Path, dir: &Path, in_use: &[String]) -> Result<u8> {
    // A lab without a readable spec can't start: nothing to place, nothing written.
    let lab = lab(dir)?;
    let _picking = lock_instances(labs)?;
    match instance(dir) {
        Some(n) if blocks_clear(Some(&lab), n, &ranges(in_use)) => Ok(n),
        _ => pick_instance(labs, dir, in_use),
    }
}

/// Held (an OS lock on `<labs>/.instances.lock`, across processes) while an instance is picked
/// and saved: two labs installing at once read the same free number otherwise.
fn lock_instances(labs: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(labs)?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(labs.join(".instances.lock"))?;
    file.lock()?;
    Ok(file)
}

/// The lowest instance number free for lab `dir` (see [`ensure_instance`]), saved in its marker.
fn pick_instance(labs: &Path, dir: &Path, in_use: &[String]) -> Result<u8> {
    let taken: Vec<u8> = std::fs::read_dir(labs)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p != dir).filter_map(|p| instance(&p)).collect())
        .unwrap_or_default();
    let lab = lab(dir).ok();
    let used = ranges(in_use);
    let n = (1..=isoloom_spec::instance::MAX)
        .find(|n| !taken.contains(n) && blocks_clear(lab.as_ref(), *n, &used))
        .ok_or_else(|| Error::Invalid("no free instance for this lab: 99 labs or networks already take its room on this machine".into()))?;
    crate::platform::atomic_file::write(&dir.join(INSTANCE_MARKER), n.to_string())?;
    Ok(n)
}

/// The IPv4 ranges of `cidrs`.
fn ranges(cidrs: &[String]) -> Vec<(u32, u32)> {
    cidrs.iter().filter_map(|c| ipv4_range(c)).collect()
}

/// Whether the Docker blocks of `lab` as instance `n` overlap none of `used`. A lab whose spec
/// can't be read has no blocks to collide; one that can't be that instance doesn't fit it.
fn blocks_clear(lab: Option<&Lab>, n: u8, used: &[(u32, u32)]) -> bool {
    let Some(lab) = lab else { return true };
    if isoloom_spec::instance::apply(&lab.spec(), n).is_err() {
        return false;
    }
    let snapshot = isoloom_targets::resolved::resolve_with(lab, Some(n));
    snapshot["networks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, net)| net["docker_cidr"].as_str().and_then(ipv4_range))
        .all(|(lo, hi)| used.iter().all(|&(a, b)| hi < a || b < lo))
}

/// An IPv4 CIDR as its first and last address.
pub fn ipv4_range(cidr: &str) -> Option<(u32, u32)> {
    let (ip, len) = cidr.split_once('/')?;
    let ip: u32 = ip.parse::<std::net::Ipv4Addr>().ok()?.into();
    let len: u32 = len.parse().ok().filter(|l| *l <= 32)?;
    let size = if len == 0 { u32::MAX } else { (1u32 << (32 - len)) - 1 };
    let lo = ip & !size;
    Some((lo, lo | size))
}
