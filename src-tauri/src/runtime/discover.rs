//! Finding labs that are actually running on this machine, by asking the engines themselves:
//! Docker for its running containers, and the hypervisors (VirtualBox, Parallels, VMware) for
//! their running VMs. Never a cache or a marker: Vagrant's global index, for one, keeps saying
//! "running" long after the VMs are gone. Each lab is deployed under a predictable name, a
//! Compose project `cyberctf-<id>` or VMs named `<lab> · <machine>` (the lab's isoloom name),
//! so one scan recovers which labs are up even after a crash or restart, when the in-memory
//! "deploying" set and the status cache are gone.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::AppHandle;
use tauri::Manager;

use crate::exec::run_read;

/// The Compose project prefix every lab's containers carry (`cyberctf-<id>`).
const PROJECT_PREFIX: &str = "cyberctf-";
/// Between the lab's name and the machine's in a VM name (`minilab · dc01`).
const VM_NAME_SEP: &str = " · ";

/// Lab ids from the Compose project labels of running containers. Input is one project name per
/// line (`docker ps --format '{{.Label "com.docker.compose.project"}}'`); containers with no such
/// label print a blank line, which is skipped. Only `cyberctf-` projects count, and an exact
/// `<id>` is taken (an attack-box or other sidecar without the label never appears).
pub(crate) fn lab_ids_from_docker_projects(out: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for line in out.lines() {
        let project = line.trim();
        if let Some(id) = project.strip_prefix(PROJECT_PREFIX)
            && is_lab_id(id)
        {
            ids.insert(id.to_string());
        }
    }
    ids.into_iter().collect()
}

/// The `name:` of an isoloom.yml: the prefix of the lab's VM names.
pub(crate) fn isoloom_name(yaml: &str) -> Option<String> {
    yaml.lines().find_map(|l| {
        let v = l.strip_prefix("name:")?.trim().trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// Installed labs by the name their VMs carry: the lab's name as its instance on this machine
/// (`<name>-<instance>`, as Isoloom generates it), from `labs/<id>/isoloom.yml`. A name two labs
/// share can't tell which one runs, so it is left out rather than guessed.
pub(crate) fn lab_names(labs_dir: &Path) -> HashMap<String, String> {
    let mut by_name: HashMap<String, Option<String>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(labs_dir) else { return HashMap::new() };
    for e in entries.flatten() {
        let Some(id) = e.file_name().to_str().map(str::to_string) else { continue };
        if !is_lab_id(&id) {
            continue;
        }
        let Some(name) = vm_name_prefix(&e.path()) else { continue };
        by_name
            .entry(name)
            .and_modify(|seen| {
                log::warn!("running-labs scan: two installed labs name their VMs alike ({id} and {seen:?})");
                *seen = None;
            })
            .or_insert(Some(id));
    }
    by_name.into_iter().filter_map(|(name, id)| Some((name, id?))).collect()
}

/// The `<lab>` of a lab's VM names (`<lab> · <machine>`): its name as this instance
/// (`<name>-<instance>`, the name `lab::instanced` gives it), or the plain `name:` of a lab with
/// no instance. Read from the spec's `name:` line, not the whole spec: the scan is polled.
fn vm_name_prefix(dir: &Path) -> Option<String> {
    let name = isoloom_name(&std::fs::read_to_string(dir.join("isoloom.yml")).ok()?)?;
    Some(match super::lab::instance(dir) {
        Some(n) => format!("{name}-{n}"),
        None => name,
    })
}

/// The lab ids of running VMs named `<lab> · <machine>`, for the labs in `by_name`. A VM of an
/// unknown lab (or one named differently) is not ours and is ignored.
pub(crate) fn lab_ids_from_vm_names<'a>(names: impl Iterator<Item = &'a str>, by_name: &HashMap<String, String>) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for name in names {
        // The launcher's own attack VM (`<lab> · attacker`) runs beside the lab; on its own it
        // doesn't mean the lab is up.
        if let Some((lab, machine)) = name.split_once(VM_NAME_SEP)
            && machine != "attacker"
            && let Some(id) = by_name.get(lab)
        {
            ids.insert(id.clone());
        }
    }
    ids.into_iter().collect()
}

/// VM names from `VBoxManage list runningvms` (`"<name>" {<uuid>}` per line).
pub(crate) fn vbox_vm_names(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix('"')?;
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// Lab ids from `vmrun list` (one running `.vmx` path per line after a count header): a lab's
/// VM lives under `.../labs/<id>/...`.
pub(crate) fn lab_ids_from_vmrun(out: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for line in out.lines() {
        if let Some(id) = id_after_labs(line.trim()) {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

/// The `<id>` in a path like `/Users/x/labs/<id>/.isoloom/vagrant/...`.
fn id_after_labs(path: &str) -> Option<String> {
    let rest = path.split("/labs/").nth(1)?;
    let id = rest.split('/').next()?;
    is_lab_id(id).then(|| id.to_string())
}

/// A plausible lab id: non-empty, bounded, and only the characters an id is built from. Keeps a
/// stray token (or an attacker project like `cyberctf-<id>-attack`, which is not an id) out.
pub(crate) fn is_lab_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The ids of labs running on this machine right now, asked of Docker and of each hypervisor.
/// Best effort: an engine that is absent or errors simply contributes nothing. Read-only and
/// timed (never hangs the caller), so it is safe to poll.
pub async fn running_lab_ids(app: &AppHandle) -> Vec<String> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    // One scan at a time: the UI polls every few seconds, and a wedged hypervisor makes each
    // probe wait out its timeout, so overlapping scans only stacked up stuck VBoxManage calls.
    // A poll during a scan gets the last answer.
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Vec<String>> = Mutex::new(Vec::new());
    if IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return LAST.lock().map(|l| l.clone()).unwrap_or_default();
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            IN_FLIGHT.store(false, Ordering::Release);
        }
    }
    let _done = Done;
    let ids = scan(app).await;
    if let Ok(mut last) = LAST.lock() {
        last.clone_from(&ids);
    }
    ids
}

async fn scan(app: &AppHandle) -> Vec<String> {
    let mut ids = BTreeSet::new();
    // Docker: the Compose project of every running container. A failure here is worth a warning
    // (the engine is missing or broken); an empty result is simply "nothing running".
    match run_read("docker", &["ps", "--filter", "label=com.docker.compose.project", "--format", "{{.Label \"com.docker.compose.project\"}}"], None).await {
        Ok(out) => ids.extend(lab_ids_from_docker_projects(&out)),
        Err(e) => log::warn!("running-labs scan: docker ps failed: {e}"),
    }
    // Hypervisors: their running VMs, named after the installed labs. Each hypervisor may simply
    // not be installed; that is not a problem for the scan.
    let by_name = app.path().app_data_dir().map(|d| lab_names(&d.join("labs"))).unwrap_or_default();
    if !by_name.is_empty() {
        match run_read("VBoxManage", &["list", "runningvms"], None).await {
            Ok(out) => {
                let names = vbox_vm_names(&out);
                ids.extend(lab_ids_from_vm_names(names.iter().map(String::as_str), &by_name));
            }
            Err(e) if e.to_string().contains("timed out") => log::warn!("running-labs scan: VirtualBox isn't responding ({e})"),
            Err(_) => {}
        }
        if let Ok(out) = run_read("prlctl", &["list", "--running", "--no-header", "-o", "name"], None).await {
            ids.extend(lab_ids_from_vm_names(out.lines().map(str::trim), &by_name));
        }
    }
    if let Ok(out) = run_read("vmrun", &["list"], None).await {
        ids.extend(lab_ids_from_vmrun(&out));
    }
    let ids: Vec<String> = ids.into_iter().collect();
    log::debug!("running-labs scan: {} running lab(s) {:?}", ids.len(), ids);
    ids
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[test]
    fn docker_projects_keep_only_cyberctf_labs_stripped_and_unique() {
        let out = "cyberctf-invoice-portal-api\n\ncyberctf-invoice-portal-api\nother-project\ncyberctf-goad-light\n";
        // Blank lines (containers with no compose label) and foreign projects are dropped; the
        // prefix is stripped and duplicates (web + database of one lab) collapse to one id.
        assert_eq!(super::lab_ids_from_docker_projects(out), vec!["goad-light".to_string(), "invoice-portal-api".to_string()]);
    }

    #[test]
    fn docker_ignores_an_attack_sidecar_without_the_compose_label() {
        // A sidecar run outside Compose prints a blank line (no project label), so it never shows.
        assert_eq!(super::lab_ids_from_docker_projects("\n\n"), Vec::<String>::new());
    }

    #[test]
    fn isoloom_name_reads_the_labs_name() {
        assert_eq!(super::isoloom_name("version: 1\nname: minilab\nnetworks:\n").as_deref(), Some("minilab"));
        assert_eq!(super::isoloom_name("name: \"goad light\"\n").as_deref(), Some("goad light"));
        assert_eq!(super::isoloom_name("version: 1\n"), None);
    }

    #[test]
    fn running_vms_map_back_to_installed_labs_by_name() {
        // Regression: the scan used to trust `vagrant global-status`, a cache that kept saying
        // "running" after the VMs were gone. Only a VM the hypervisor lists, named after an
        // installed lab, counts.
        let by_name: HashMap<String, String> = [("minilab".to_string(), "bf08e551".to_string())].into();
        let out = "\"WindowsServer2019_1791271622558_72151\" {a4dc}\n\"minilab · dc01\" {0185}\n\"minilab · ws01\" {3eeb}\n\"other · box\" {1111}\n";
        let names = super::vbox_vm_names(out);
        assert_eq!(names.len(), 4);
        // Two VMs of one lab collapse to one id; a box import and an unknown lab's VM are ignored.
        assert_eq!(super::lab_ids_from_vm_names(names.iter().map(String::as_str), &by_name), vec!["bf08e551".to_string()]);
    }

    #[test]
    fn vmrun_paths_name_the_lab_by_its_folder() {
        let out = "Total running VMs: 1\n/Users/x/Library/App/labs/goad-light/.isoloom/vagrant/.vagrant/machines/dc01/vmware_desktop/x.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(out), vec!["goad-light".to_string()]);
        assert_eq!(super::lab_ids_from_vmrun("Total running VMs: 0\n"), Vec::<String>::new());
    }

    #[test]
    fn installed_labs_are_found_by_their_isoloom_name() {
        let labs = std::env::temp_dir().join(format!("cyberctf-discover-{}", rand::random::<u32>()));
        assert!(super::lab_names(&labs).is_empty(), "no labs folder yet");
        for (id, yaml) in [("minilab-1", Some("name: minilab\n")), ("no-name", Some("version: 1\n")), ("no-spec", None), ("bad id!", Some("name: x\n"))] {
            let d = labs.join(id);
            std::fs::create_dir_all(&d).unwrap();
            if let Some(y) = yaml {
                std::fs::write(d.join("isoloom.yml"), y).unwrap();
            }
        }
        let by_name = super::lab_names(&labs);
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name["minilab"], "minilab-1");
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn vms_are_matched_by_the_labs_instance_name() {
        // Regression: the scan keyed labs by their raw isoloom name, but their VMs are named
        // after the lab's instance (`minilab-2 · dc01`), so after a restart a VirtualBox or
        // Parallels lab was never found running.
        const SPEC: &str = "version: 1\nname: minilab\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  dc01:\n    networks: { lab: 10 }\n    docker: { image: nginx:1.27 }\n";
        let labs = std::env::temp_dir().join(format!("cyberctf-discover-inst-{}", rand::random::<u32>()));
        for (id, instance) in [("goad-a", Some("1")), ("goad-b", Some("2")), ("twin-1", None), ("twin-2", None)] {
            let d = labs.join(id);
            std::fs::create_dir_all(&d).unwrap();
            let spec = if id.starts_with("twin") { SPEC.replace("minilab", "twin") } else { SPEC.to_string() };
            std::fs::write(d.join("isoloom.yml"), spec).unwrap();
            if let Some(n) = instance {
                std::fs::write(d.join(crate::runtime::lab::INSTANCE_MARKER), n).unwrap();
            }
        }
        let by_name = super::lab_names(&labs);
        // The very name Isoloom gives the instance.
        assert_eq!(crate::runtime::lab::instanced(&labs.join("goad-b")).map(|s| s.name).map_err(|e| e.to_string()), Ok("minilab-2".to_string()));
        // Two copies of one lab, told apart by their instance.
        assert_eq!(by_name.get("minilab-1").map(String::as_str), Some("goad-a"));
        assert_eq!(by_name.get("minilab-2").map(String::as_str), Some("goad-b"));
        // Two labs whose VMs would carry the same name: neither is guessed.
        assert!(!by_name.contains_key("twin"));
        let running = ["minilab-2 · dc01", "twin · dc01"];
        assert_eq!(super::lab_ids_from_vm_names(running.into_iter(), &by_name), ["goad-b".to_string()]);
        std::fs::remove_dir_all(labs).unwrap();
    }
}
