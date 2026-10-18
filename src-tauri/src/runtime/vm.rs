use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{LabStatus, Machine};
use crate::error::Result;
use crate::platform::exec::{Vagrant, run, run_env_timed};

/// A status read must never hang the status poll: a wedged VirtualBox (its global lock held by
/// a stuck VBoxManage) would otherwise pile up one blocked `vagrant status` per poll tick.
const STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Isoloom's provisioning controller: the machine that runs Ansible against the others. Not a
/// target, so it is powered off once the lab is built, and booted again to provision or check.
pub const CONTROLLER: &str = super::lab::CONTROLLER;

/// Destroys the VMs so the next start restores the lab's initial state. `env` must carry
/// the same server connection as `start`: Vagrant re-evaluates the Vagrantfile.
pub async fn stop(dir: &Path, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let destroyed = Vagrant::of(dir, env).stream(&["destroy", "--force"], &mut log).await;
    // Vagrant only destroys what it still tracks. A lab re-installed from its registered commit
    // loses `.vagrant/` (Vagrant's machine index), so the VMs of an earlier run become orphans
    // Vagrant reports as `not_created` and leaves behind — "Stop & clean up" then looked like it
    // did nothing. Finish at the hypervisor, scoped to this lab's exact VM names, for every
    // hypervisor we know how to clean (each is a no-op where the names don't exist).
    let names: HashSet<String> = vm_names(dir).into_iter().collect();
    if !names.is_empty() {
        recover_virtualbox(&names, &mut log).await;
        recover_parallels(&names, &mut log).await;
    }
    destroyed
}

mod recover;
mod status;

#[cfg(test)]
use recover::{folder_candidates, machine_folder, ours_in_prlctl, parse_machine_folder, parse_vbox_vms, remove_leftover_folders};
pub(super) use recover::{has_machines, recover_parallels, recover_virtualbox, vm_names, vm_names_from};
pub use status::status;
#[cfg(test)]
use status::{cidr, field, parse_status, quoted, status_from, vagrant_topology};

#[cfg(test)]
mod tests;
