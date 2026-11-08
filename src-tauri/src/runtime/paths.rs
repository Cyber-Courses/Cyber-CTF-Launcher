//! Where a lab's files live (its folder, its Terraform state) and the small marker files the
//! launcher leaves next to them to remember how and where the lab was started.

use std::path::{Path, PathBuf};

use crate::platform::AppDirs;

use super::Runtime;
use super::model::Park;
use super::providers::Provider;
use crate::error::{Error, Result};

pub fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok { Ok(()) } else { Err(Error::Invalid(format!("invalid lab id `{id}`"))) }
}

/// The Isothrum runner's home: the app data folder, its runs kept under `runs/` beside `labs/`.
pub(crate) fn runner_home(dirs: &AppDirs) -> Result<PathBuf> {
    Ok(dirs.data()?.to_path_buf())
}

/// An installed lab's folder.
pub(crate) fn lab_dir(dirs: &AppDirs, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    lab_dir_in(dirs.data()?, id)
}

/// [`lab_dir`] under the app data folder `base` (the id already validated).
pub(super) fn lab_dir_in(base: &Path, id: &str) -> Result<PathBuf> {
    let dir = base.join("labs").join(id);
    if !dir.is_dir() {
        return Err(Error::Invalid(format!("lab `{id}` is not installed")));
    }
    Ok(dir)
}

/// The ids with a Terraform deployment under the app data folder `base` (valid ids only): the
/// labs the expired-lab reaper looks at.
pub(crate) fn deployed_ids(base: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(base.join("deployments")) else { return Vec::new() };
    entries.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|id| validate_id(id).is_ok()).collect()
}

/// Every Terraform state folder of lab `id` under the app data folder `base`, by target
/// (`aws`, `proxmox`, …): the current deployment's and any left from an earlier one elsewhere.
pub(crate) fn deployment_states(base: &Path, id: &str) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(base.join("deployments").join(id)) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> =
        entries.flatten().filter(|e| e.path().is_dir()).filter_map(|e| Some((e.file_name().into_string().ok()?, e.path()))).collect();
    out.sort();
    out
}

/// The lab's vagrant folders (Docker on one VM, one VM per machine) that hold a Vagrantfile, i.e.
/// that a local or ESXi VM of it may have been started from.
pub(super) fn vagrant_dirs(dir: &Path) -> impl Iterator<Item = PathBuf> {
    [Runtime::Docker, Runtime::Vm].into_iter().map(|rt| super::lab::vagrant_dir(dir, rt)).filter(|v| v.join("Vagrantfile").exists())
}

/// Next to a lab's Terraform state: which runtime started it (`DOCKER`, `VM` or `CLOUD`), for
/// the auto-stop reaper.
const RUNTIME_FILE: &str = "runtime";

fn runtime_from_tag(tag: &str) -> Runtime {
    match tag.trim() {
        "VM" => Runtime::Vm,
        "CLOUD" => Runtime::Cloud,
        _ => Runtime::Docker,
    }
}

/// The runtime recorded at start; containers when nothing (readable) was.
pub(super) fn recorded_runtime(state: &Path) -> Runtime {
    std::fs::read_to_string(state.join(RUNTIME_FILE)).map_or(Runtime::Docker, |t| runtime_from_tag(&t))
}

/// Next to a lab's Terraform state: the host (server or cloud account id) it was applied on, so
/// the state can still be destroyed with that host's credentials after the lab moved elsewhere.
const HOST_FILE: &str = "host";

/// The host recorded with a Terraform state, if any (states from older versions have none).
pub(crate) fn recorded_host(state: &Path) -> Option<String> {
    std::fs::read_to_string(state.join(HOST_FILE)).ok().map(|h| h.trim().to_string()).filter(|h| !h.is_empty())
}

/// Writes `value` to `dir/name`, or removes the file for `None` (already gone is fine). A marker
/// that can't be cleared is an error: left behind, it would send every later status, stop and
/// check to the wrong form.
fn set_marker(dir: &Path, name: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => std::fs::write(dir.join(name), v)?,
        None => match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        },
    }
    Ok(())
}

/// Marks a container lab as running in a VM on this machine (Isoloom's docker-vm),
/// so stop, status and the attack-box shell go to the VM instead of local Docker.
pub const LOCAL_VM_MARKER: &str = ".cyberctf-local-vm";

pub(super) fn mark_local_vm(dir: &Path, provider: Option<Provider>) -> Result<()> {
    set_marker(dir, LOCAL_VM_MARKER, provider.map(super::providers::Provider::id))
}

/// The hypervisor a container lab runs on in a local VM, if it does.
pub(crate) fn local_vm(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(LOCAL_VM_MARKER)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Marks a container lab as started as one VM per machine on this machine (Isoloom's vagrant
/// target, the form of a VM lab): the player's pick in "Where should it run?" for a lab whose
/// spec has VM recipes too. Status, stop, park, provisioning and the attack box then follow the
/// VM form (see [`local_runtime`]), whatever the catalogue calls the lab.
pub const PER_MACHINE_MARKER: &str = ".cyberctf-per-machine-vm";

pub(crate) fn mark_per_machine_vm(dir: &Path, on: bool) -> Result<()> {
    set_marker(dir, PER_MACHINE_MARKER, on.then_some("vm"))
}

/// Whether a container lab runs (or ran last) as one VM per machine on this machine.
pub(crate) fn per_machine_vm(dir: &Path) -> bool {
    dir.join(PER_MACHINE_MARKER).is_file()
}

/// The form lab `dir` runs in on this machine: `VM` for a container lab started as one VM per
/// machine ([`PER_MACHINE_MARKER`]), else its own `runtime`. Every operation on a lab here goes
/// through this, so the webview can keep naming the lab by its catalogue runtime.
pub(crate) fn local_runtime(dir: &Path, runtime: Runtime) -> Runtime {
    if runtime == Runtime::Docker && per_machine_vm(dir) { Runtime::Vm } else { runtime }
}

/// The Docker context a container lab here was started on (`orbstack`, `desktop-linux`, …), when
/// the player chose an engine other than the current one. Every docker/compose call for the lab —
/// the launcher's and the runner's — then goes to that context, so status, stop, park, checks, the
/// attack box and the shell stay on the engine the lab runs on even if the current context later
/// changes. Nothing written = the current context, as before.
pub const ENGINE_MARKER: &str = ".cyberctf-engine";

pub(super) fn mark_engine(dir: &Path, context: Option<&str>) -> Result<()> {
    set_marker(dir, ENGINE_MARKER, context)
}

/// The Docker context a container lab here runs on, if one was recorded at launch.
pub(crate) fn engine(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(ENGINE_MARKER)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The host keys of a lab's own hosts that Vagrant runs (a lab VM here, a lab host on ESXi), for
/// its attack box shell. Kept with the lab, not shared: another lab later given the same address
/// (DHCP, a recycled IP) has another key, and a shared pin refused every shell to it.
const LAB_KNOWN_HOSTS: &str = ".cyberctf-known_hosts";

pub(super) fn lab_known_hosts(dir: &Path) -> PathBuf {
    dir.join(LAB_KNOWN_HOSTS)
}

/// Forgets the keys pinned for a lab's hosts: a new deployment brings new hosts, maybe at the
/// address of another one.
pub(super) fn forget_lab_host_keys(dir: &Path) {
    let _ = std::fs::remove_file(lab_known_hosts(dir));
}

/// Written when the launcher parks a lab (paused or shut down), so a later start resumes its
/// machines instead of clearing them as leftovers; removed on resume and on stop.
const PARKED_MARKER: &str = ".cyberctf-parked";

pub(super) fn mark_parked(dir: &Path, mode: Option<Park>) -> Result<()> {
    set_marker(dir, PARKED_MARKER, mode.map(super::model::Park::id))
}

pub(super) fn parked(dir: &Path) -> Option<Park> {
    std::fs::read_to_string(dir.join(PARKED_MARKER)).ok().and_then(|s| Park::from_id(&s))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_marker_that_cannot_be_cleared_is_an_error_and_one_already_gone_is_not() {
        let dir = std::env::temp_dir().join(format!("cyberctf-marker-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        super::set_marker(&dir, "m", None).expect("nothing to clear");
        // A directory where the marker would be: remove_file refuses it.
        std::fs::create_dir(dir.join("m")).unwrap();
        assert!(super::set_marker(&dir, "m", None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::{
        PARKED_MARKER, Park, Provider, Runtime, deployed_ids, lab_dir_in, local_vm, mark_local_vm, mark_parked, parked, recorded_runtime, vagrant_dirs,
        validate_id,
    };

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn local_vm_marker_round_trips() {
        let dir = temp_dir("cyberctf-localvm");
        assert_eq!(local_vm(&dir), None);
        mark_local_vm(&dir, Some(Provider::Virtualbox)).unwrap();
        assert_eq!(local_vm(&dir).as_deref(), Some("virtualbox"));
        mark_local_vm(&dir, None).unwrap();
        assert_eq!(local_vm(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_container_lab_started_as_vms_runs_as_a_vm_lab_here() {
        use super::{local_runtime, mark_per_machine_vm, per_machine_vm};
        let dir = temp_dir("cyberctf-permachine");
        assert!(!per_machine_vm(&dir));
        // Without the mark, every runtime is its own.
        for rt in [Runtime::Docker, Runtime::Vm, Runtime::Cloud] {
            assert_eq!(local_runtime(&dir, rt), rt);
        }
        mark_per_machine_vm(&dir, true).unwrap();
        assert!(per_machine_vm(&dir));
        // A container lab follows the VM form; a VM lab already does, a cloud lab never.
        assert_eq!(local_runtime(&dir, Runtime::Docker), Runtime::Vm);
        assert_eq!(local_runtime(&dir, Runtime::Vm), Runtime::Vm);
        assert_eq!(local_runtime(&dir, Runtime::Cloud), Runtime::Cloud);
        mark_per_machine_vm(&dir, false).unwrap();
        assert!(!per_machine_vm(&dir));
        assert_eq!(local_runtime(&dir, Runtime::Docker), Runtime::Docker);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn engine_marker_round_trips_and_reads_trimmed() {
        use super::{ENGINE_MARKER, engine, mark_engine};
        let dir = temp_dir("cyberctf-engine");
        assert_eq!(engine(&dir), None);
        mark_engine(&dir, Some("orbstack")).unwrap();
        assert_eq!(engine(&dir).as_deref(), Some("orbstack"));
        // A trailing newline changes nothing; blank is none.
        std::fs::write(dir.join(ENGINE_MARKER), "desktop-linux\n").unwrap();
        assert_eq!(engine(&dir).as_deref(), Some("desktop-linux"));
        std::fs::write(dir.join(ENGINE_MARKER), "  \n").unwrap();
        assert_eq!(engine(&dir), None);
        mark_engine(&dir, Some("colima")).unwrap();
        mark_engine(&dir, None).unwrap();
        assert_eq!(engine(&dir), None);
        assert!(!dir.join(ENGINE_MARKER).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parked_marker_round_trips_and_ignores_garbage() {
        let dir = temp_dir("cyberctf-parked");
        assert_eq!(parked(&dir), None);
        mark_parked(&dir, Some(Park::Pause)).unwrap();
        assert_eq!(parked(&dir), Some(Park::Pause));
        mark_parked(&dir, Some(Park::Shutdown)).unwrap();
        assert_eq!(parked(&dir), Some(Park::Shutdown));
        // A marker nothing wrote (or a corrupt one) never counts as parked: start then cleans up.
        std::fs::write(dir.join(PARKED_MARKER), "halted?").unwrap();
        assert_eq!(parked(&dir), None);
        mark_parked(&dir, None).unwrap();
        assert_eq!(parked(&dir), None);
        assert!(!dir.join(PARKED_MARKER).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn markers_are_read_trimmed_and_unknown_values_fall_back() {
        use super::{LOCAL_VM_MARKER, recorded_host};
        let dir = temp_dir("cyberctf-markers");
        // A trailing newline (a hand edit, a tool that adds one) changes nothing; blank is none.
        std::fs::write(dir.join(LOCAL_VM_MARKER), "virtualbox\n").unwrap();
        assert_eq!(local_vm(&dir).as_deref(), Some("virtualbox"));
        std::fs::write(dir.join(LOCAL_VM_MARKER), "  \n").unwrap();
        assert_eq!(local_vm(&dir), None);
        std::fs::write(dir.join(PARKED_MARKER), "pause\n").unwrap();
        assert_eq!(parked(&dir), Some(Park::Pause));
        std::fs::write(dir.join(PARKED_MARKER), "PAUSE").unwrap();
        assert_eq!(parked(&dir), None, "exact ids only");
        std::fs::write(dir.join("host"), "  aws1 \n").unwrap();
        assert_eq!(recorded_host(&dir).as_deref(), Some("aws1"));
        std::fs::write(dir.join("host"), "\n").unwrap();
        assert_eq!(recorded_host(&dir), None);
        // The runtime tag is exact too: an unknown one reads as containers (the oldest kind).
        std::fs::write(dir.join("runtime"), "vm").unwrap();
        assert_eq!(recorded_runtime(&dir), Runtime::Docker);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_path_traversal_and_shell_characters() {
        for bad in ["", "../x", "a/b", "a b", "a;rm", "é", &"x".repeat(65)] {
            assert!(validate_id(bad).is_err(), "{bad:?} should be rejected");
        }
        for good in ["web-1", "sqli_basic", "3f2a9c1e-7b1d-4c4e-9a53-2d1c8f0e6b7a"] {
            assert!(validate_id(good).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn a_lab_folder_must_be_installed() {
        let base = temp_dir("cyberctf-appdata");
        let err = lab_dir_in(&base, "web-1").unwrap_err().to_string();
        assert_eq!(err, "lab `web-1` is not installed");
        std::fs::create_dir_all(base.join("labs").join("web-1")).unwrap();
        assert_eq!(lab_dir_in(&base, "web-1").unwrap(), base.join("labs").join("web-1"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn deployed_ids_skip_names_that_are_not_lab_ids() {
        let base = temp_dir("cyberctf-deployed");
        assert!(deployed_ids(&base).is_empty(), "no deployments folder yet");
        for name in ["web-1", "sqli_basic", "not a lab", ".hidden"] {
            std::fs::create_dir_all(base.join("deployments").join(name)).unwrap();
        }
        let mut ids = deployed_ids(&base);
        ids.sort();
        assert_eq!(ids, ["sqli_basic", "web-1"]);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn vagrant_dirs_lists_only_folders_with_a_vagrantfile() {
        let dir = temp_dir("cyberctf-vagrantdirs");
        assert_eq!(vagrant_dirs(&dir).count(), 0);
        let vm = crate::runtime::lab::vagrant_dir(&dir, Runtime::Vm);
        std::fs::create_dir_all(&vm).unwrap();
        assert_eq!(vagrant_dirs(&dir).count(), 0, "a folder without a Vagrantfile is no VM");
        std::fs::write(vm.join("Vagrantfile"), "").unwrap();
        assert_eq!(vagrant_dirs(&dir).collect::<Vec<_>>(), std::slice::from_ref(&vm));
        let docker_vm = crate::runtime::lab::vagrant_dir(&dir, Runtime::Docker);
        std::fs::create_dir_all(&docker_vm).unwrap();
        std::fs::write(docker_vm.join("Vagrantfile"), "").unwrap();
        assert_eq!(vagrant_dirs(&dir).collect::<Vec<_>>(), [docker_vm, vm]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
