use std::path::{Path, PathBuf};

use super::{LabStatus, Machine};
use crate::error::Result;
use crate::platform::exec::run_env_timed;

/// A status read must never hang the status poll: a wedged VirtualBox (its global lock held by
/// a stuck `VBoxManage`) would otherwise pile up one blocked `vagrant status` per poll tick.
const STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Isoloom's provisioning controller: the machine that runs Ansible against the others. Not a
/// target, so it is powered off once the lab is built, and booted again to provision or check.
pub const CONTROLLER: &str = super::lab::CONTROLLER;

mod records;
mod status;

pub(crate) use records::has_machines;
pub use status::status;
#[cfg(test)]
use status::{cidr, field, parse_status, quoted, status_from, vagrant_topology, vm_names_from};

#[cfg(test)]
mod tests;
