//! Local lab VMs running on this machine, from `vagrant global-status`.
//!
//! This reads Vagrant's cached state, which can say "running" for a VM that a crash or a host
//! reboot has actually stopped, so the "running now" count it feeds may over-report until Vagrant
//! next refreshes. It is never pruned (`--prune`): pruning reads every VM's state live, and on
//! Hyper-V that takes each VM's Vagrant lock for a second or two, so a lab's own Vagrant command
//! landing then was refused (`MachineActionLockedError`) and its start failed. A record whose
//! folder is gone, the stale kind pruning dropped, is skipped instead. The effect is cosmetic only: the count and the per-lab Stop button are the
//! harm, and that Stop merely runs `vagrant destroy -f` (a no-op on an already-down VM); nothing
//! here drives automatic cleanup. The live, never-cached "which labs are up" answer is
//! [`crate::runtime::discover`], asked of each hypervisor directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::Workload;
use crate::error::{Error, Result};
use crate::machine::selftest;
use crate::platform::AppDirs;
use crate::platform::exec::run_env_timed;
use crate::runtime::paths::labs_root as labs_dir;

/// `vagrant global-status` talks to VirtualBox, which can be wedged: past this it is given up.
const GLOBAL_STATUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Providers whose machines run elsewhere (a server host), not on this machine.
const REMOTE_PROVIDERS: [&str; 2] = ["vmware_esxi", "proxmox"];

/// How long a `vagrant global-status` answer stands: the sidebar and the Machine page each poll
/// the workloads every few seconds, and each scan is a Vagrant (Ruby) start, a second or two of CPU
/// on Windows. Vagrant's answer is its own cache anyway (see above).
const GLOBAL_STATUS_FRESH: Duration = Duration::from_secs(30);

/// `vagrant global-status`, one at a time and at most every [`GLOBAL_STATUS_FRESH`]: a poll that
/// lands while the previous scan is still running, or soon after it, reuses its answer instead of
/// starting another vagrant (they contend for one lock, so overlapping scans only get slower and
/// pile up).
async fn global_status() -> Option<String> {
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    let last = || LAST.lock().unwrap_or_else(PoisonError::into_inner).clone();

    // Unit tests script each scan's answer per test: one test's must never stand for another's.
    if !cfg!(test)
        && let Some(answer) = fresh(last(), Instant::now())
    {
        return Some(answer);
    }
    if IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return last().map(|(_, answer)| answer);
    }
    // Cleared on drop too, so a cancelled scan doesn't block every later one.
    let _done = crate::platform::ResetOnDrop(&IN_FLIGHT);
    // Timed: without a limit a polled call would hang and stack up one blocked process per tick.
    let out = run_env_timed("vagrant", &["global-status", "--machine-readable"], None, &[], GLOBAL_STATUS_TIMEOUT).await.ok();
    if let Some(o) = &out {
        *LAST.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), o.clone()));
    }
    out
}

/// The `last` scan's answer, when it is still fresh at `now`.
fn fresh(last: Option<(Instant, String)>, now: Instant) -> Option<String> {
    last.filter(|(at, _)| now.saturating_duration_since(*at) < GLOBAL_STATUS_FRESH).map(|(_, answer)| answer)
}

/// One machine in `vagrant global-status`.
#[derive(Default, Debug, PartialEq)]
struct Machine {
    provider: String,
    home: String,
    state: String,
}

/// The machines in `vagrant global-status --machine-readable`: each record is consecutive lines
/// (`machine-id`, `provider-name`, `machine-home`, `state`, ...).
fn machines(out: &str) -> Vec<Machine> {
    let mut recs: Vec<Machine> = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.splitn(4, ',').collect();
        if parts.len() < 4 {
            continue;
        }
        if parts[2] == "machine-id" {
            recs.push(Machine::default());
            continue;
        }
        let Some(r) = recs.last_mut() else { continue };
        let value = parts[3].to_string();
        match parts[2] {
            "provider-name" => r.provider = value,
            "machine-home" => r.home = value,
            "state" => r.state = value,
            _ => {}
        }
    }
    recs
}

/// The workload a machine belongs to: `selftest`, the lab whose folder holds it, or none (not
/// one of the app's own).
fn owner(home: &Path, labs: Option<&Path>, selftest_dir: Option<&Path>) -> Option<String> {
    if selftest_dir == Some(home) {
        return Some("selftest".to_string());
    }
    let lab = labs.and_then(|l| home.strip_prefix(l).ok()).and_then(|rel| rel.components().next())?;
    Some(lab.as_os_str().to_string_lossy().into_owned())
}

/// Running local VMs from `vagrant global-status`, kept to the app's own folders.
pub(super) async fn workloads(dirs: &AppDirs) -> Vec<Workload> {
    let Some(out) = global_status().await else {
        return Vec::new();
    };
    let labs = labs_dir(dirs).ok();
    let selftest_dir = selftest::work_dir(dirs, "vm").ok();
    running_by_owner(&out, labs.as_deref(), selftest_dir.as_deref())
}

/// The app's running local VMs in `vagrant global-status` output, one workload per lab; a record
/// whose folder is gone is stale.
fn running_by_owner(out: &str, labs: Option<&Path>, selftest_dir: Option<&Path>) -> Vec<Workload> {
    let mut by_lab: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let live = |r: &Machine| r.state == "running" && !REMOTE_PROVIDERS.contains(&r.provider.as_str()) && Path::new(&r.home).is_dir();
    for r in machines(out).into_iter().filter(live) {
        let Some(id) = owner(Path::new(&r.home), labs, selftest_dir) else { continue };
        let e = by_lab.entry(id).or_insert((0, r.provider.clone()));
        e.0 += 1;
    }
    by_lab.into_iter().map(|(id, (count, provider))| Workload { id, kind: "vm", count, mem_bytes: 0, provider: Some(provider) }).collect()
}

/// Upgrade only: the folder beside `.isoloom` where older versions of the launcher kept the
/// attack VM, a Vagrant project of its own (`<lab>/.cyberctf-attack-vm`). Nothing makes it any
/// more (the box is a machine of the lab now); a stop still destroys what an older version left.
const LEGACY_ATTACK_VM_DIR: &str = ".cyberctf-attack-vm";

/// Destroys a lab's (or the self-test's) local VMs.
pub(super) async fn stop(dirs: &AppDirs, id: &str) -> Result<()> {
    // A lab's VMs: one per machine (.isoloom/vagrant) or Docker on one VM (.isoloom/docker-vm).
    let vagrant_dirs: Vec<PathBuf> = if id == "selftest" {
        vec![selftest::work_dir(dirs, "vm")?]
    } else {
        let lab = labs_dir(dirs)?.join(id);
        let vagrant_dir = |runtime| crate::runtime::lab::vagrant_dir(&lab, runtime);
        // And an older version's attack VM ([`LEGACY_ATTACK_VM_DIR`]), so a lab stopped after an
        // upgrade does not leave it behind. The Vagrantfile filter below skips it when there is
        // none.
        vec![vagrant_dir(crate::runtime::Runtime::Vm), vagrant_dir(crate::runtime::Runtime::Docker), lab.join(LEGACY_ATTACK_VM_DIR)]
    };
    let vagrant_dirs: Vec<PathBuf> = vagrant_dirs.into_iter().filter(|d| d.join("Vagrantfile").is_file()).collect();
    if vagrant_dirs.is_empty() {
        return Err(Error::Invalid(format!("no VMs found for `{id}`")));
    }
    // Every folder is attempted even after one fails (a lab's own destroy failing must not leave
    // the others behind); the first failure is what the stop reports once all were tried.
    let mut first_failure = None;
    for dir in vagrant_dirs {
        if let Err(e) = crate::platform::exec::Vagrant::of(&dir, &[]).run(&["destroy", "-f"]).await {
            first_failure.get_or_insert(e);
        }
    }
    first_failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_stands_for_a_while_so_polls_dont_start_vagrant_each_time() {
        let at = Instant::now();
        let last = || Some((at, "1,default,state,running".to_string()));
        assert_eq!(fresh(last(), at + GLOBAL_STATUS_FRESH.saturating_sub(Duration::from_secs(1))).as_deref(), Some("1,default,state,running"));
        assert_eq!(fresh(last(), at + GLOBAL_STATUS_FRESH), None, "scanned again once old");
        assert_eq!(fresh(None, at), None);
    }

    #[test]
    fn reads_machines_from_global_status() {
        let out = "1,,machine-id,abc\n1,,provider-name,virtualbox\n1,,machine-home,/data/labs/goad/.isoloom/vagrant\n1,,state,running\n\
                   2,,machine-id,def\n2,,provider-name,vmware_esxi\n2,,state,running\n2,,ui,info,ignored\n";
        let m = machines(out);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Machine { provider: "virtualbox".into(), home: "/data/labs/goad/.isoloom/vagrant".into(), state: "running".into() });
        assert_eq!(m[1].provider, "vmware_esxi");
        // Lines before the first machine are ignored.
        assert_eq!(machines("1,,state,running\n"), [] as [Machine; 0]);
    }

    #[test]
    fn a_machine_belongs_to_its_labs_folder_or_the_selftest() {
        let labs = Path::new("/data/labs");
        let selftest = Path::new("/cache/selftest/vm");
        assert_eq!(owner(Path::new("/data/labs/goad/.isoloom/vagrant"), Some(labs), Some(selftest)).as_deref(), Some("goad"));
        assert_eq!(owner(selftest, Some(labs), Some(selftest)).as_deref(), Some("selftest"));
        assert_eq!(owner(Path::new("/home/me/other"), Some(labs), Some(selftest)), None);
    }

    #[test]
    fn records_without_a_home_or_outside_the_labs_folder_are_not_workloads() {
        let t = crate::testing::test_app::TestApp::new();
        let labs = t.data().join("labs");
        let selftest = t.data().join("selftest").join("vm");
        std::fs::create_dir_all(labs.join("goad").join(".isoloom").join("vagrant")).unwrap();
        std::fs::create_dir_all(labs.join("goad").join(super::LEGACY_ATTACK_VM_DIR)).unwrap();
        let home = |p: &Path| p.display().to_string();
        // The labs folder itself (no lab under it), a record with no home line (a remote
        // machine's), a sibling folder whose name only starts like the labs folder, and a lab
        // VM that is saved rather than running.
        let out = format!(
            "1,,machine-id,a\n1,,provider-name,virtualbox\n1,,machine-home,{}\n1,,state,running\n\
             2,,machine-id,b\n2,,provider-name,virtualbox\n2,,state,running\n\
             3,,machine-id,c\n3,,provider-name,virtualbox\n3,,machine-home,{}2/goad/.isoloom/vagrant\n3,,state,running\n\
             4,,machine-id,d\n4,,provider-name,virtualbox\n4,,machine-home,{}\n4,,state,saved\n",
            home(&labs),
            home(&labs),
            home(&labs.join("goad").join(".isoloom").join("vagrant"))
        );
        assert!(running_by_owner(&out, Some(&labs), Some(&selftest)).is_empty());
        // A folder with a Vagrantfile under a lab, however deep, belongs to that lab.
        let deep = format!(
            "5,,machine-id,e\n5,,provider-name,qemu\n5,,machine-home,{}\n5,,state,running\n",
            home(&labs.join("goad").join(super::LEGACY_ATTACK_VM_DIR))
        );
        let w = running_by_owner(&deep, Some(&labs), Some(&selftest));
        assert_eq!(w.iter().map(|w| (w.id.as_str(), w.count)).collect::<Vec<_>>(), [("goad", 1)]);
        // A record whose folder is gone (a lab removed without its VMs destroyed) is stale: the
        // index isn't pruned, which would read every VM's state and take its lock.
        let gone = format!(
            "6,,machine-id,f\n6,,provider-name,hyperv\n6,,machine-home,{}\n6,,state,running\n",
            home(&labs.join("removed").join(".isoloom").join("vagrant"))
        );
        assert!(running_by_owner(&gone, Some(&labs), Some(&selftest)).is_empty());
    }

    #[tokio::test]
    async fn stopping_a_lab_destroys_each_vagrant_folder_it_has() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let lab = t.lab("goad", &[]);
        // Docker on one VM and one VM per machine both left something: each folder is destroyed.
        for sub in ["vagrant", "docker-vm"] {
            let d = crate::runtime::lab::out(&lab).join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("Vagrantfile"), "").unwrap();
        }
        // And an older version's attack VM beside `.isoloom`: destroyed with the lab's.
        let attack = lab.join(super::LEGACY_ATTACK_VM_DIR);
        std::fs::create_dir_all(&attack).unwrap();
        std::fs::write(attack.join("Vagrantfile"), "").unwrap();
        let tools = Tools::rules(&[]);
        super::stop(&t.dirs(), "goad").await.unwrap();
        assert_eq!(tools.calls().iter().filter(|c| c.as_str() == "vagrant destroy -f").count(), 3, "{:?}", tools.calls());
    }

    #[test]
    fn only_the_apps_running_local_vms_count() {
        let t = crate::testing::test_app::TestApp::new();
        let labs = t.data().join("labs");
        let selftest = t.data().join("selftest").join("vm");
        let home = |p: &Path| p.display().to_string();
        let goad = labs.join("goad").join(".isoloom").join("vagrant");
        for dir in [&goad, &selftest] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let goad = home(&goad);
        let out = format!(
            "1,,machine-id,a\n1,,provider-name,virtualbox\n1,,machine-home,{goad}\n1,,state,running\n\
             2,,machine-id,b\n2,,provider-name,virtualbox\n2,,machine-home,{goad}\n2,,state,running\n\
             3,,machine-id,c\n3,,provider-name,virtualbox\n3,,machine-home,{goad}\n3,,state,poweroff\n\
             4,,machine-id,d\n4,,provider-name,vmware_esxi\n4,,machine-home,{goad}\n4,,state,running\n\
             5,,machine-id,e\n5,,provider-name,qemu\n5,,machine-home,{}\n5,,state,running\n\
             6,,machine-id,f\n6,,provider-name,qemu\n6,,machine-home,/elsewhere\n6,,state,running\n",
            home(&selftest)
        );
        let w = running_by_owner(&out, Some(&labs), Some(&selftest));
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].id.as_str(), w[0].count, w[0].provider.as_deref()), ("goad", 2, Some("virtualbox")));
        assert_eq!((w[1].id.as_str(), w[1].count, w[1].kind), ("selftest", 1, "vm"));
        assert!(running_by_owner(&out, None, None).is_empty());
    }

    /// The first `vagrant destroy -f` failing (a locked VirtualBox, a VM half-removed by hand)
    /// used to end the stop there, leaving the other folders running.
    /// Each folder is still attempted, and the stop reports that first failure once all were.
    #[tokio::test]
    async fn a_failed_destroy_still_destroys_the_other_folders_and_is_reported() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let t = crate::testing::test_app::TestApp::new();
        let lab = t.lab("goad", &[]);
        let out = crate::runtime::lab::out(&lab);
        for d in [out.join("vagrant"), out.join("docker-vm"), lab.join(super::LEGACY_ATTACK_VM_DIR)] {
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("Vagrantfile"), "").unwrap();
        }
        let mut destroys = 0;
        let tools = Tools::new(move |_, args| {
            if args.first() != Some(&"destroy") {
                return ok("");
            }
            destroys += 1;
            if destroys == 1 { fail("VBoxManage: error: The object is not ready") } else { ok("") }
        });
        let err = super::stop(&t.dirs(), "goad").await.unwrap_err().to_string();
        assert!(err.contains("The object is not ready"), "{err}");
        assert_eq!(tools.calls().iter().filter(|c| c.as_str() == "vagrant destroy -f").count(), 3, "{:?}", tools.calls());
    }
}
