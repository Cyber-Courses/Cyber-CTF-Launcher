//! A real VM lab through the launcher's runner path, end to end: generate the lab's files and
//! start it on a local hypervisor (`launch::start_vm_run`, the path the lab page takes with the
//! Isothrum runner chosen), read its status, run its checks, pause and resume the lab, shut it
//! down and resume it, provision it again, then stop it (`lifecycle::stop_run`) and assert
//! nothing of it is left. The attack box among a VM lab's is `attack_box`'s. Opt-in (it
//! needs Vagrant, a hypervisor and its boxes):
//!
//! ```text
//! CYBERCTF_TEST_VM_LAB=<a VM lab: one Linux machine publishing an HTTP port, with checks> \
//! CYBERCTF_TEST_VM_PROVIDER=virtualbox \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::vm::a_vm_lab_runs_end_to_end_through_the_runner
//! ```
//!
//! `CYBERCTF_TEST_VM_INPUTS` gives the launch's values (`NAME=value,…`).

use std::path::{Path, PathBuf};

use super::support::{check, install_copy, log, vm_inputs};
use crate::runtime::providers::Provider;
use crate::runtime::{Park, Place, isothrum, launch, lifecycle};

/// One VM lab installed for the test.
struct Lab {
    dir: PathBuf,
    id: String,
    home: PathBuf,
    provider: Provider,
}

impl Lab {
    fn run(&self) -> isothrum::LabRun {
        isothrum::LabRun::vm(&self.home, &self.dir, &self.id).unwrap()
    }
}

/// Its status: running on a local VM, its machines up, and its entry answering here.
async fn up_and_answering(l: &Lab, f: &mut Vec<String>, when: &str) {
    match l.run().status().await {
        Ok(s) => {
            check(s.running && matches!(s.place, Some(Place::LocalVm)), format!("{when}: it runs on a local VM"), f);
            for m in s.machines.iter().filter(|m| !m.infra) {
                check(m.state == "running", format!("{when}: machine {} is running (state {})", m.name, m.state), f);
            }
            let Some(url) = s.url else { return check(false, format!("{when}: it has a local URL"), f) };
            let res = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().get(&url).send().await;
            check(res.as_ref().is_ok_and(|r| r.status().is_success()), format!("{when}: {url} answers ({:?})", res.map(|r| r.status())), f);
        }
        Err(e) => check(false, format!("{when}: its status reads: {e}"), f),
    }
}

/// Parks the lab in `mode`, checks it reads as parked, resumes it and checks it is back.
async fn park_and_resume(l: &Lab, mode: Park, f: &mut Vec<String>) {
    let parked = l.run().park(mode, &mut log(&l.id)).await;
    check(parked.is_ok(), format!("it parks ({mode:?}): {:?}", parked.err()), f);
    crate::runtime::paths::mark_parked(&l.dir, Some(mode)).unwrap();
    let s = l.run().status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked == Some(mode)), format!("parked ({mode:?}), it reads as parked"), f);
    let resumed = l.run().resume(&mut log(&l.id)).await;
    check(resumed.is_ok(), format!("it resumes after {mode:?}: {:?}", resumed.err()), f);
    crate::runtime::paths::mark_parked(&l.dir, None).unwrap();
    up_and_answering(l, f, &format!("resumed after {mode:?}")).await;
}

/// The lab while it exists; every failure is collected so the lab is always stopped after.
async fn exercise(l: &Lab, f: &mut Vec<String>) {
    if let Err(e) = launch::start_vm_run(&l.home, &l.dir, &l.id, launch::VmStart { provider: l.provider, env: &vm_inputs() }, &mut log(&l.id)).await {
        return check(false, format!("it starts: {e}"), f);
    }
    up_and_answering(l, f, "started").await;
    match l.run().check().await {
        Ok(c) => {
            check(
                c.available && c.ok,
                format!("its checks pass ({:?}: {})", c.results.iter().map(|r| (&r.name, r.ok)).collect::<Vec<_>>(), c.output.trim()),
                f,
            );
        }
        Err(e) => check(false, format!("its checks run: {e}"), f),
    }
    // vagrant-qemu has no `suspend`: a QEMU lab shuts down and resumes, never pauses.
    if l.provider != Provider::Qemu {
        park_and_resume(l, Park::Pause, f).await;
    }
    park_and_resume(l, Park::Shutdown, f).await;
    let provisioned = l.run().provision(None, &mut log(&l.id)).await;
    check(provisioned.is_ok(), format!("it provisions again: {:?}", provisioned.err()), f);
}

/// How a VM of a lab shows in a hypervisor's listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shows {
    /// Named `<lab> · <machine>` (VirtualBox, Parallels, UTM).
    Labelled,
    /// Named `<lab><sep><machine>` (libvirt `_`, Hyper-V `-`).
    Prefixed(char),
    /// By a path in the lab's folder (VMware's `.vmx`, a QEMU process's disk).
    InFolder,
}

/// How `provider` lists every VM it holds (running or not, where it can say), and how a lab's
/// shows there; none for a provider the scenario can't check (a server's, which isn't local).
fn listing(provider: Provider) -> Option<(&'static str, &'static [&'static str], Shows)> {
    Some(match provider {
        Provider::Virtualbox => ("VBoxManage", &["list", "vms"], Shows::Labelled),
        Provider::Parallels => ("prlctl", &["list", "--all", "--no-header", "-o", "name"], Shows::Labelled),
        Provider::Utm => ("utmctl", &["list"], Shows::Labelled),
        Provider::Libvirt => ("virsh", &["-c", "qemu:///system", "list", "--all", "--name"], Shows::Prefixed('_')),
        Provider::Hyperv => ("powershell", &["-NoProfile", "-Command", "Get-VM | ForEach-Object { $_.Name }"], Shows::Prefixed('-')),
        // vmrun lists running VMs only, by their .vmx, which Vagrant keeps in the lab's folder.
        Provider::VmwareDesktop => ("vmrun", &["list"], Shows::InFolder),
        // vagrant-qemu's VMs are QEMU processes, started on disks in the lab's folder.
        Provider::Qemu => ("ps", &["-axo", "command"], Shows::InFolder),
        _ => return None,
    })
}

/// Whether `line` of a listing is a VM of the lab named `lab` in `folder`, shown as `shows`.
fn is_labs(line: &str, shows: Shows, lab: &str, folder: &str) -> bool {
    match shows {
        Shows::Labelled => line.contains(&format!("{lab} · ")),
        Shows::Prefixed(sep) => line.trim().starts_with(&format!("{lab}{sep}")),
        Shows::InFolder => line.contains(folder),
    }
}

/// The VMs of the lab named `lab`, installed in `dir`, that the hypervisor `provider` still holds.
///
/// # Errors
///
/// When it can't be told: a provider without a listing here, or one whose listing failed. Never
/// "nothing left" by default.
async fn left_on(provider: Provider, lab: &str, dir: &Path) -> Result<Vec<String>, String> {
    let (program, args, shows) = listing(provider).ok_or_else(|| format!("can't check what is left on {}", provider.id()))?;
    let out = crate::platform::exec::run(program, args, None).await.map_err(|e| format!("listing the VMs on {} failed: {e}", provider.id()))?;
    let folder = dir.display().to_string();
    Ok(out.lines().filter(|l| is_labs(l, shows, lab, &folder)).map(str::to_owned).collect())
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_VM_LAB)"]
async fn a_vm_lab_runs_end_to_end_through_the_runner() {
    let Ok(src) = std::env::var("CYBERCTF_TEST_VM_LAB") else { return };
    let provider: Provider =
        serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "virtualbox".into()))).unwrap();
    let id = format!("vm-{}", rand::random::<u16>());
    let dir = install_copy(Path::new(&src), &id);
    let home = std::env::temp_dir().join(format!("cyberctf-runner-{id}"));
    let lab = Lab { dir, id, home, provider };
    let mut failures = Vec::new();
    exercise(&lab, &mut failures).await;
    let stopped = lifecycle::stop_run(isothrum::LabRun::vm(&lab.home, &lab.dir, &lab.id).unwrap(), &lab.dir, log(&lab.id)).await;
    check(stopped.is_ok(), format!("it stops: {:?}", stopped.err()), &mut failures);
    let name = crate::runtime::lab::lab(&lab.dir).map(|l| l.source().name.clone()).unwrap_or_default();
    let left = left_on(provider, &name, &lab.dir).await;
    check(left.as_ref().is_ok_and(Vec::is_empty), format!("nothing of it is left on {} ({left:?})", provider.id()), &mut failures);
    check(lab.run().status().await.is_ok_and(|s| !s.running && s.machines.is_empty()), "its status reads as gone".into(), &mut failures);
    let _ = std::fs::remove_dir_all(&lab.dir);
    let _ = std::fs::remove_dir_all(&lab.home);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_local_provider_is_checked_for_leftovers_and_a_server_never_passes_by_default() {
        // Regression: only VirtualBox and libvirt were listed; on any other hypervisor the check
        // found nothing and passed.
        for p in Provider::ALL {
            assert_eq!(listing(p).is_some(), !p.is_remote(), "{p:?}");
        }
        let folder = "/data/labs/web";
        let shows = |p| listing(p).unwrap().2;
        assert!(is_labs("\"Web · srv\" {1234}", shows(Provider::Virtualbox), "Web", folder));
        assert!(!is_labs("\"Webshop · srv\" {1234}", shows(Provider::Virtualbox), "Web", folder));
        assert!(is_labs("Web · srv", shows(Provider::Parallels), "Web", folder));
        assert!(is_labs("1234-5678  stopped  Web · srv", shows(Provider::Utm), "Web", folder));
        assert!(is_labs("Web_srv", shows(Provider::Libvirt), "Web", folder));
        assert!(!is_labs("Webshop_srv", shows(Provider::Libvirt), "Web", folder));
        assert!(is_labs("Web-srv\r", shows(Provider::Hyperv), "Web", folder));
        assert!(is_labs("/data/labs/web/.vagrant/machines/srv/vmware_desktop/x/srv.vmx", shows(Provider::VmwareDesktop), "Web", folder));
        assert!(is_labs("qemu-system-aarch64 -drive file=/data/labs/web/.vagrant/machines/srv/qemu/disk.qcow2", shows(Provider::Qemu), "Web", folder));
        assert!(!is_labs("qemu-system-aarch64 -drive file=/data/labs/other/disk.qcow2", shows(Provider::Qemu), "Web", folder));
    }

    #[tokio::test]
    async fn a_listing_that_fails_or_a_server_is_an_error_not_nothing_left() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let _tools = Tools::rules(&[("VBoxManage list vms", ok("\"Web · srv\" {1}\n\"Other · a\" {2}\n")), ("virsh", fail("not found"))]);
        let dir = Path::new("/data/labs/web");
        assert_eq!(left_on(Provider::Virtualbox, "Web", dir).await.unwrap(), ["\"Web · srv\" {1}"]);
        assert!(left_on(Provider::Libvirt, "Web", dir).await.unwrap_err().contains("listing the VMs on libvirt failed"));
        assert!(left_on(Provider::VmwareEsxi, "Web", dir).await.unwrap_err().contains("can't check"));
    }
}
