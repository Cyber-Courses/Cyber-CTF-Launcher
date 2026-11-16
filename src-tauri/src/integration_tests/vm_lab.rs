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
//! cargo test --lib -- --ignored --nocapture --test-threads=1 --exact integration_tests::vm_lab::a_vm_lab_runs_end_to_end_through_the_runner
//! ```
//!
//! `CYBERCTF_TEST_VM_INPUTS` gives the launch's values (`NAME=value,…`).

use std::path::{Path, PathBuf};

use super::support::lab::{install_copy, vm_inputs};
use super::support::{check, fetch, log, scenario};
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
    /// Its run. Opened at every step, as the lab page opens it: once the start opened it, opening
    /// it again doesn't fail (and a panic would be the scenario's failed check, its teardown run).
    fn run(&self) -> isothrum::LabRun {
        isothrum::LabRun::vm(&self.home, &self.dir, &self.id).expect("the lab's run opens")
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
            let res = fetch(&url).await;
            check(res.as_ref().is_ok_and(reqwest::StatusCode::is_success), format!("{when}: {url} answers ({res:?})"), f);
        }
        Err(e) => check(false, format!("{when}: its status reads: {e}"), f),
    }
}

/// Parks the lab in `mode`, checks it reads as parked, resumes it and checks it is back.
async fn park_and_resume(l: &Lab, mode: Park, f: &mut Vec<String>) {
    let parked = l.run().park(mode, &mut log(&l.id)).await;
    check(parked.is_ok(), format!("it parks ({mode:?}): {:?}", parked.err()), f);
    let marked = crate::runtime::paths::mark_parked(&l.dir, Some(mode));
    check(marked.is_ok(), format!("parked ({mode:?}), the lab is marked so ({:?})", marked.err()), f);
    let s = l.run().status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked == Some(mode)), format!("parked ({mode:?}), it reads as parked"), f);
    let resumed = l.run().resume(&mut log(&l.id)).await;
    check(resumed.is_ok(), format!("it resumes after {mode:?}: {:?}", resumed.err()), f);
    let unmarked = crate::runtime::paths::mark_parked(&l.dir, None);
    check(unmarked.is_ok(), format!("resumed after {mode:?}, its parked mark goes ({:?})", unmarked.err()), f);
    up_and_answering(l, f, &format!("resumed after {mode:?}")).await;
}

/// The lab while it exists; every failure is collected so the lab is always stopped after.
async fn exercise(l: &Lab, f: &mut Vec<String>) {
    if let Err(e) =
        launch::start_vm_run(&l.home, &l.dir, &l.id, launch::VmStart { provider: l.provider, inputs: launch::Inputs::values(&vm_inputs()) }, &mut log(&l.id))
            .await
    {
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
/// utmctl ships inside UTM's bundle, on no PATH.
fn listing(provider: Provider) -> Option<(&'static str, &'static [&'static str], Shows)> {
    Some(match provider {
        Provider::Virtualbox => ("VBoxManage", &["list", "vms"], Shows::Labelled),
        Provider::Parallels => ("prlctl", &["list", "--all", "--no-header", "-o", "name"], Shows::Labelled),
        Provider::Utm => (crate::runtime::providers::utmctl(), &["list"], Shows::Labelled),
        Provider::Libvirt => ("virsh", &["-c", "qemu:///system", "list", "--all", "--name"], Shows::Prefixed('_')),
        Provider::Hyperv => ("powershell", &["-NoProfile", "-Command", "Get-VM | ForEach-Object { $_.Name }"], Shows::Prefixed('-')),
        // vmrun lists running VMs only, by their .vmx, which Vagrant keeps in the lab's folder.
        Provider::VmwareDesktop => ("vmrun", &["list"], Shows::InFolder),
        // vagrant-qemu's VMs are QEMU processes, started on disks in the lab's folder.
        Provider::Qemu => ("ps", &["-axo", "command"], Shows::InFolder),
        _ => return None,
    })
}

/// Whether `line` of a listing is a VM of the lab whose VMs are named with `stem`
/// (`<name>-<instance>-<key>`, [`crate::runtime::discover::vm_stem`]), installed in `folder`, shown
/// as `shows`: `<stem> - <machine>` (VirtualBox, Parallels, UTM), `<stem>_<machine>` (libvirt),
/// `<stem>-<machine>` (Hyper-V), or by a path in its folder.
fn is_labs(line: &str, shows: Shows, stem: &str, folder: &str) -> bool {
    match shows {
        Shows::Labelled => line.contains(&format!("{stem} - ")),
        Shows::Prefixed(sep) => line.trim().starts_with(&format!("{stem}{sep}")),
        Shows::InFolder => line.contains(folder),
    }
}

/// The VMs of the lab whose VMs are named with `stem`, installed in `dir`, that the hypervisor
/// `provider` still holds.
///
/// # Errors
///
/// When it can't be told: a provider without a listing here, or one whose listing failed. Never
/// "nothing left" by default.
async fn left_on(provider: Provider, stem: &str, dir: &Path) -> Result<Vec<String>, String> {
    let (program, args, shows) = listing(provider).ok_or_else(|| format!("can't check what is left on {}", provider.id()))?;
    let out = crate::platform::exec::run(program, args, None).await.map_err(|e| format!("listing the VMs on {} failed: {e}", provider.id()))?;
    let folder = dir.display().to_string();
    Ok(out.lines().filter(|l| is_labs(l, shows, stem, &folder)).map(str::to_owned).collect())
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_VM_LAB)"]
async fn a_vm_lab_runs_end_to_end_through_the_runner() {
    let Some(src) = super::support::knob("CYBERCTF_TEST_VM_LAB") else { return };
    let provider = super::support::vm_provider(Provider::Virtualbox);
    let id = format!("vm-{}", rand::random::<u16>());
    let dir = install_copy(Path::new(&src), &id);
    if super::support::refused_by_launcher(&dir, crate::runtime::lab::Target::Vagrant, provider) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let home = std::env::temp_dir().join(format!("cyberctf-runner-{id}"));
    let lab = Lab { dir, id, home, provider };
    let mut failures = Vec::new();
    scenario(
        &mut failures,
        async |f: &mut Vec<String>| exercise(&lab, f).await,
        async |f: &mut Vec<String>| {
            let stopped = match isothrum::LabRun::vm(&lab.home, &lab.dir, &lab.id) {
                Ok(run) => lifecycle::stop_run(run, &lab.dir, log(&lab.id)).await,
                Err(e) => Err(e),
            };
            check(stopped.is_ok(), format!("it stops: {:?}", stopped.err()), f);
            // Read before anything goes: the stem is the spec's name and the run's key.
            let left = match crate::runtime::discover::vm_stem(&lab.dir, &lab.id, &lab.home) {
                Some(stem) => left_on(provider, &stem, &lab.dir).await,
                None => Err("the lab's VM names can't be told: its spec names none".to_owned()),
            };
            check(left.as_ref().is_ok_and(Vec::is_empty), format!("nothing of it is left on {} ({left:?})", provider.id()), f);
            check(lab.run().status().await.is_ok_and(|s| !s.running && s.machines.is_empty()), "its status reads as gone".into(), f);
        },
    )
    .await;
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
        // As Isoloom 3.0 names a copy's VMs: the stem (`<name>-<key>`), then the machine.
        let stem = "web-k3y9ab";
        assert!(is_labs("\"web-k3y9ab - srv\" {1234}", shows(Provider::Virtualbox), stem, folder));
        assert!(!is_labs("\"web-zz9zz9 - srv\" {1234}", shows(Provider::Virtualbox), stem, folder), "another copy's");
        assert!(!is_labs("\"webshop-k3y9ab - srv\" {1234}", shows(Provider::Virtualbox), stem, folder));
        assert!(is_labs("web-k3y9ab - srv", shows(Provider::Parallels), stem, folder));
        assert!(is_labs("1234-5678  stopped  web-k3y9ab - srv", shows(Provider::Utm), stem, folder));
        assert!(is_labs("web-k3y9ab_srv", shows(Provider::Libvirt), stem, folder));
        assert!(!is_labs("webshop-k3y9ab_srv", shows(Provider::Libvirt), stem, folder));
        assert!(is_labs("web-k3y9ab-srv\r", shows(Provider::Hyperv), stem, folder));
        assert!(is_labs("/data/labs/web/.vagrant/machines/srv/vmware_desktop/x/srv.vmx", shows(Provider::VmwareDesktop), stem, folder));
        assert!(is_labs("qemu-system-aarch64 -drive file=/data/labs/web/.vagrant/machines/srv/qemu/disk.qcow2", shows(Provider::Qemu), stem, folder));
        assert!(!is_labs("qemu-system-aarch64 -drive file=/data/labs/other/disk.qcow2", shows(Provider::Qemu), stem, folder));
    }

    #[test]
    fn a_leftover_is_found_by_the_name_isoloom_gives_it_with_utms_own_tool() {
        // Regression: the check looked for `<name> · <machine>`, a name 3.0 never gives: it found
        // nothing on VirtualBox, Parallels or UTM, so "nothing is left" passed whatever was left.
        let labelled = listing(Provider::Virtualbox).unwrap().2;
        assert!(!is_labs("\"web-k3y9ab - srv\" {1234}", labelled, "web · ", "/data/labs/web"));
        assert!(is_labs("\"web-k3y9ab - srv\" {1234}", labelled, "web-k3y9ab", "/data/labs/web"));
        // UTM's tool from inside its bundle, wherever UTM is: it's on no PATH ("not installed").
        assert!(listing(Provider::Utm).unwrap().0.ends_with("UTM.app/Contents/MacOS/utmctl"));
    }

    #[tokio::test]
    async fn a_listing_that_fails_or_a_server_is_an_error_not_nothing_left() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let _tools = Tools::strict_rules(&[("VBoxManage list vms", ok("\"web-k3y9ab - srv\" {1}\n\"other-k3y9ab - a\" {2}\n")), ("virsh", fail("not found"))]);
        let dir = Path::new("/data/labs/web");
        assert_eq!(left_on(Provider::Virtualbox, "web-k3y9ab", dir).await.unwrap(), ["\"web-k3y9ab - srv\" {1}"]);
        assert!(left_on(Provider::Libvirt, "web-k3y9ab", dir).await.unwrap_err().contains("listing the VMs on libvirt failed"));
        assert!(left_on(Provider::VmwareEsxi, "web-k3y9ab", dir).await.unwrap_err().contains("can't check"));
    }
}
