//! The VM an earlier test left behind: one Vagrant no longer knows (its `.vagrant` state gone,
//! say with the app's data folder at an uninstall) keeps running from the test folder, holds its
//! disk, and the next `vagrant up` fails on it ("the file is being used by another process" on
//! Hyper-V, "a machine with this name already exists" on VirtualBox). It is found by the test VM's
//! fixed name and removed, but only when it is surely this test's: on Hyper-V every disk of it
//! is under the test folder; on VirtualBox its description names the test folder (set by the
//! test's Vagrantfile). A VM of that name that is anyone else's is left alone, and said.

use std::path::Path;

use super::VM_NAME;
use crate::error::{Error, Result};
use crate::platform::exec::{Leftover, remove_hyperv_leftover, run};

/// What a test VM's VirtualBox description says: it is the test's in `dir`.
pub(super) fn vbox_description(dir: &Path) -> String {
    format!("cyberctf-selftest:{}", dir.display())
}

/// Removes a test VM an earlier run left on any hypervisor that can be asked by name. What was
/// removed, for the test's report (empty: nothing was left).
///
/// # Errors
///
/// A VM of the test's name that isn't this test's (left alone), or a removal that failed: the
/// test can't boot its own VM until it is gone.
pub(super) async fn clear(dir: &Path) -> Result<Vec<&'static str>> {
    let mut removed = Vec::new();
    // Hyper-V asks for administrator rights: only when its disk folder says a VM was left.
    if dir.join(".vagrant").join("machines").join("default").join("hyperv").join("Virtual Hard Disks").is_dir() {
        found("Hyper-V", remove_hyperv_leftover(VM_NAME, dir).await?, &mut removed)?;
    }
    found("VirtualBox", remove_vbox_leftover(dir).await?, &mut removed)?;
    Ok(removed)
}

/// Adds `hypervisor` to `removed` when its leftover was removed; a foreign VM is an error.
fn found(hypervisor: &'static str, leftover: Leftover, removed: &mut Vec<&'static str>) -> Result<()> {
    match leftover {
        Leftover::None => Ok(()),
        Leftover::Removed => {
            log::info!("removed the VM an earlier self-test left on {hypervisor}");
            removed.push(hypervisor);
            Ok(())
        }
        Leftover::Foreign => Err(Error::Invalid(format!(
            "{hypervisor} has a VM named \"{VM_NAME}\" that isn't this test's, so it was left alone. Rename or remove it in {hypervisor}, then test again."
        ))),
    }
}

/// [`clear`] on VirtualBox: its VMs of the test's exact name, by `VBoxManage`. None when
/// VirtualBox isn't here.
async fn remove_vbox_leftover(dir: &Path) -> Result<Leftover> {
    let Ok(list) = run("VBoxManage", &["list", "vms"], None).await else { return Ok(Leftover::None) };
    let ids = vbox_ids_named(&list, VM_NAME);
    if ids.is_empty() {
        return Ok(Leftover::None);
    }
    let ours = vbox_description(dir);
    for id in &ids {
        let info = run("VBoxManage", &["showvminfo", id, "--machinereadable"], None).await?;
        if vbox_value(&info, "description").as_deref() != Some(ours.as_str()) {
            return Ok(Leftover::Foreign);
        }
    }
    for id in &ids {
        // A VM that isn't running refuses the power-off: only the removal must work.
        if let Err(e) = run("VBoxManage", &["controlvm", id, "poweroff"], None).await {
            log::debug!("powering off the leftover test VM {id}: {e}");
        }
        run("VBoxManage", &["unregistervm", id, "--delete"], None).await?;
    }
    Ok(Leftover::Removed)
}

/// The ids of the VMs named exactly `name` in `VBoxManage list vms` (`"name" {uuid}` lines).
fn vbox_ids_named(list: &str, name: &str) -> Vec<String> {
    let quoted = format!("\"{name}\" ");
    list.lines().filter_map(|l| l.trim().strip_prefix(&quoted)).map(|id| id.trim().trim_matches(['{', '}']).to_string()).collect()
}

/// The value of `key` in `VBoxManage showvminfo --machinereadable` (`key="value"` lines).
fn vbox_value(info: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=\"");
    info.lines().find_map(|l| l.trim().strip_prefix(&prefix)?.strip_suffix('"').map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};

    #[test]
    fn a_vm_is_found_by_its_exact_name_and_its_description_read() {
        let list = "\"CyberCTF test VM\" {11111111-2222}\n\"CyberCTF test VM 2\" {3333}\n\"lab · web\" {4444}\n";
        assert_eq!(vbox_ids_named(list, VM_NAME), ["11111111-2222"]);
        assert_eq!(vbox_ids_named("", VM_NAME), [] as [String; 0]);
        let info = "name=\"CyberCTF test VM\"\ndescription=\"cyberctf-selftest:/tmp/vm\"\nVMState=\"running\"\n";
        assert_eq!(vbox_value(info, "description").as_deref(), Some("cyberctf-selftest:/tmp/vm"));
        assert_eq!(vbox_value(info, "missing"), None);
    }

    #[tokio::test]
    async fn the_tests_own_leftover_is_removed_and_anyone_elses_left_alone() {
        let dir = Path::new("/data/selftest/vm");
        let ours = format!("description=\"{}\"\n", vbox_description(dir));
        let tools = Tools::rules(&[
            ("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")),
            ("VBoxManage showvminfo abc", ok(&ours)),
            ("VBoxManage controlvm abc poweroff", fail("not running")),
        ]);
        assert_eq!(clear(dir).await.unwrap(), ["VirtualBox"]);
        assert!(tools.called("VBoxManage unregistervm abc --delete"), "{:?}", tools.calls());

        let tools = Tools::rules(&[("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")), ("VBoxManage showvminfo abc", ok("description=\"\"\n"))]);
        let err = clear(dir).await.unwrap_err().to_string();
        assert!(err.contains("isn't this test's, so it was left alone"), "{err}");
        assert!(!tools.called("unregistervm") && !tools.called("controlvm"), "{:?}", tools.calls());

        let _none = Tools::rules(&[("VBoxManage list vms", ok("\"lab · web\" {x}\n"))]);
        assert_eq!(clear(dir).await.unwrap(), [] as [&str; 0]);
        let _no_vbox = Tools::rules(&[("VBoxManage", fail("not found"))]);
        assert!(clear(dir).await.unwrap().is_empty(), "no VirtualBox here");
    }
}
