//! Local lab VMs running on this machine, from `vagrant global-status`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::AppHandle;

use super::Workload;
use crate::error::{Error, Result};
use crate::exec::{run, run_env_timed};
use crate::machine::{labs_dir, selftest};

/// `vagrant global-status` talks to VirtualBox, which can be wedged: past this it is given up.
const GLOBAL_STATUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Providers whose machines run elsewhere (a server host), not on this machine.
const REMOTE_PROVIDERS: [&str; 2] = ["vmware_esxi", "proxmox"];

/// `vagrant global-status`, one at a time: a poll that lands while the previous scan is still
/// running reuses the last answer instead of starting another vagrant (they contend for one lock,
/// so overlapping scans only get slower and pile up).
async fn global_status() -> Option<String> {
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Option<String>> = Mutex::new(None);

    if IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return LAST.lock().ok().and_then(|l| l.clone());
    }
    // Cleared on drop too, so a cancelled scan doesn't block every later one.
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            IN_FLIGHT.store(false, Ordering::Release);
        }
    }
    let _done = Done;
    // Timed: without a limit a polled call would hang and stack up one blocked process per tick.
    let out = run_env_timed("vagrant", &["global-status", "--prune", "--machine-readable"], None, &[], GLOBAL_STATUS_TIMEOUT).await.ok();
    if let (Some(o), Ok(mut last)) = (&out, LAST.lock()) {
        *last = Some(o.clone());
    }
    out
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
pub(super) async fn workloads(app: &AppHandle) -> Vec<Workload> {
    let Some(out) = global_status().await else {
        return Vec::new();
    };
    let labs = labs_dir(app).ok();
    let selftest_dir = selftest::work_dir(app, "vm").ok();
    running_by_owner(&out, labs.as_deref(), selftest_dir.as_deref())
}

/// The app's running local VMs in `vagrant global-status` output, one workload per lab.
fn running_by_owner(out: &str, labs: Option<&Path>, selftest_dir: Option<&Path>) -> Vec<Workload> {
    let mut by_lab: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for r in machines(out).into_iter().filter(|r| r.state == "running" && !REMOTE_PROVIDERS.contains(&r.provider.as_str())) {
        let Some(id) = owner(Path::new(&r.home), labs, selftest_dir) else { continue };
        let e = by_lab.entry(id).or_insert((0, r.provider.clone()));
        e.0 += 1;
    }
    by_lab.into_iter().map(|(id, (count, provider))| Workload { id, kind: "vm", count, mem_bytes: 0, provider: Some(provider) }).collect()
}

/// Destroys a lab's (or the self-test's) local VMs.
pub(super) async fn stop(app: &AppHandle, id: &str) -> Result<()> {
    // A lab's VMs: one per machine (.isoloom/vagrant) or Docker on one VM (.isoloom/docker-vm).
    let dirs: Vec<PathBuf> = if id == "selftest" {
        vec![selftest::work_dir(app, "vm")?]
    } else {
        let out = crate::runtime::lab::out(&labs_dir(app)?.join(id));
        vec![out.join("vagrant"), out.join("docker-vm")]
    };
    let dirs: Vec<PathBuf> = dirs.into_iter().filter(|d| d.join("Vagrantfile").is_file()).collect();
    if dirs.is_empty() {
        return Err(Error::Invalid(format!("no VMs found for `{id}`")));
    }
    for dir in dirs {
        run("vagrant", &["destroy", "-f"], Some(&dir)).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_machines_from_global_status() {
        let out = "1,,machine-id,abc\n1,,provider-name,virtualbox\n1,,machine-home,/data/labs/goad/.isoloom/vagrant\n1,,state,running\n\
                   2,,machine-id,def\n2,,provider-name,vmware_esxi\n2,,state,running\n2,,ui,info,ignored\n";
        let m = machines(out);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Machine { provider: "virtualbox".into(), home: "/data/labs/goad/.isoloom/vagrant".into(), state: "running".into() });
        assert_eq!(m[1].provider, "vmware_esxi");
        // Lines before the first machine are ignored.
        assert!(machines("1,,state,running\n").is_empty());
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
    fn only_the_apps_running_local_vms_count() {
        let labs = std::env::temp_dir().join("labs");
        let selftest = std::env::temp_dir().join("selftest").join("vm");
        let home = |p: &Path| p.display().to_string();
        let goad = home(&labs.join("goad").join(".isoloom").join("vagrant"));
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
}
