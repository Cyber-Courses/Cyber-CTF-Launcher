//! Finding labs that are actually running on this machine, by asking the engines themselves:
//! Docker for its running containers, and the hypervisors (VirtualBox, Parallels, VMware, UTM,
//! libvirt and Hyper-V) for their running VMs. Never a cache or a marker: Vagrant's global index,
//! for one, keeps saying "running" long after the VMs are gone. Each lab is deployed under a
//! predictable name, a Compose project `cyberctf-<id>` or VMs named for the lab's copy on this
//! machine (`<stem> - <machine>` on VirtualBox/Parallels/UTM, `<stem>_<machine>` on libvirt,
//! `<stem>-<machine>` on Hyper-V, the stem being `<lab>-<key>`, the lab's name and its run's key),
//! so one scan recovers which labs are up even after a crash or restart, when the in-memory
//! "deploying" set and the status cache are gone. The key is the runner's for the lab's run, so
//! another copy of the same lab (a test's, another app's) never reads as this one running.
//!
//! Not covered: VMs that vagrant-qemu runs directly, with no libvirt and no registry to list them
//! (a bare `qemu-system` process per VM). They would have to be found by scanning the process table
//! for a `qemu-system` whose disk path is under `labs/<id>/.isoloom/`, which the engine's own source
//! does not guarantee (the vagrant-qemu gem spawns it), so it is left out rather than guessed at.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use super::is_lab_id;
use crate::platform::AppDirs;
use crate::platform::exec::run_read;

/// Between the copy's stem and the machine's name in a VM's display name (`minilab-1-k3x9a2 -
/// dc01`), as Isoloom writes it.
const VM_NAME_SEP: &str = " - ";

/// Lab ids from the Compose project labels of running containers. Input is one project name per
/// line (`docker ps --format '{{.Label "com.docker.compose.project"}}'`); containers with no such
/// label print a blank line, which is skipped. Only `cyberctf-` projects count, and an exact
/// `<id>` is taken (an attack-box or other sidecar without the label never appears).
pub(crate) fn lab_ids_from_docker_projects(out: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for line in out.lines() {
        let project = line.trim();
        if let Some(id) = super::docker::lab_of_project(project)
            && is_lab_id(id)
        {
            ids.insert(id.to_string());
        }
    }
    ids.into_iter().collect()
}

/// The `name:` of an isoloom.yml, the prefix of the lab's VM names, as a YAML parser reads it
/// (Isoloom's, so the VM names', quoting and comments included). Only that key is read: the scan
/// is polled, and the rest of the spec isn't validated.
pub(crate) fn isoloom_name(yaml: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Named {
        name: Option<String>,
    }
    serde_yaml_ng::from_str::<Named>(yaml).ok()?.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty())
}

/// Installed labs by the stem their VMs' names start with: the lab's name as its instance on this
/// machine (`<name>-<instance>`, as Isoloom generates it, from `labs/<id>/isoloom.yml`), then the
/// key of the lab's run on the runner whose home is `home` (`<name>-<instance>-<key>`, as Isoloom
/// names a copy's VMs). A stem two labs share can't tell which one runs, so it is left out rather
/// than guessed.
pub(crate) fn lab_stems(labs_dir: &Path, home: &Path) -> HashMap<String, String> {
    let mut by_name: HashMap<String, Option<String>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(labs_dir) else { return HashMap::new() };
    for e in entries.flatten() {
        let Some(id) = e.file_name().to_str().map(str::to_string) else { continue };
        if !is_lab_id(&id) {
            continue;
        }
        let Some(prefix) = vm_name_prefix(&e.path()) else { continue };
        let Ok(run) = isothrum_runner::RunName::new(&super::docker::project(&id)) else { continue };
        let name = format!("{prefix}-{}", isothrum_runner::copy_key(home, &run));
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

/// The start of a lab's VM names' stem (`<lab>-<key>`): its name as this instance
/// (`<name>-<instance>`, the name Isoloom's `instance::apply` gives it), or the plain `name:`
/// of a lab with no instance. Read from the spec's `name:` alone, not the whole spec: the scan is
/// polled.
fn vm_name_prefix(dir: &Path) -> Option<String> {
    let name = isoloom_name(&std::fs::read_to_string(dir.join("isoloom.yml")).ok()?)?;
    Some(match super::lab::instance(dir) {
        Some(n) => format!("{name}-{n}"),
        None => name,
    })
}

/// The lab ids of running VMs named `<stem> - <machine>`, for the labs whose stems are in
/// `by_name`. A VM of an unknown lab, of another copy, or named differently is not ours and is
/// ignored.
pub(crate) fn lab_ids_from_vm_names<'a>(names: impl Iterator<Item = &'a str>, by_name: &HashMap<String, String>) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for name in names {
        if let Some((lab, _)) = name.split_once(VM_NAME_SEP)
            && let Some(id) = by_name.get(lab)
        {
            ids.insert(id.clone());
        }
    }
    ids.into_iter().collect()
}

/// The lab ids of running VMs named `<stem><sep><machine>`, for the labs whose stems are in
/// `by_name`: libvirt domains (`<stem>_<machine>`, from vagrant-libvirt's `default_prefix`) and
/// Hyper-V VMs (`<stem>-<machine>`). An ESXi host's VMs aren't this machine's: never scanned.
/// Unlike the ` - ` separator, `_` and `-` can occur inside a stem, so a lab is matched as a
/// prefix, not by splitting: the longest installed stem that leaves a non-empty machine wins.
pub(crate) fn lab_ids_from_prefixed_vm_names<'a>(names: impl Iterator<Item = &'a str>, by_name: &HashMap<String, String>, sep: char) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for name in names {
        let name = name.trim();
        // The most specific (longest) installed name that prefixes this VM and leaves a machine.
        // Longest first matters when one lab's name is another's with a `<sep>` tail (`goad` and
        // `goad-light`): `goad-light-dc01` is read as goad-light's dc01, not goad's `light-dc01`.
        let best = by_name
            .iter()
            .filter_map(|(lab, id)| {
                let machine = name.strip_prefix(lab.as_str())?.strip_prefix(sep)?;
                (!machine.is_empty()).then_some((lab.len(), id, machine))
            })
            .max_by_key(|&(len, _, _)| len);
        if let Some((_, id, _)) = best {
            ids.insert(id.clone());
        }
    }
    ids.into_iter().collect()
}

/// One VM of a `VBoxManage list vms` or `list runningvms` listing.
pub(crate) struct VboxVm {
    /// The VM's name, as VirtualBox shows it.
    pub(crate) name: String,
    /// Its uuid, without the braces around it.
    pub(crate) id: String,
}

/// The VMs of a `VBoxManage list vms` or `list runningvms` listing (`"<name>" {<uuid>}` per
/// line); a line that isn't a quoted name is skipped. The one reader of that listing.
pub(crate) fn vbox_vms(out: &str) -> Vec<VboxVm> {
    out.lines()
        .filter_map(|l| {
            let (name, id) = l.trim().strip_prefix('"')?.split_once('"')?;
            Some(VboxVm { name: name.to_string(), id: id.trim().trim_matches(['{', '}']).to_string() })
        })
        .collect()
}

/// VM names from `VBoxManage list runningvms` ([`vbox_vms`]).
pub(crate) fn vbox_vm_names(out: &str) -> Vec<String> {
    vbox_vms(out).into_iter().map(|vm| vm.name).collect()
}

/// Names of the started VMs from `utmctl list` (`<uuid>  <status>  <name>`, under a `UUID Status
/// Name` header). Only `started` VMs count. A UTM VM's name is the lab's `<stem> - <machine>` label
/// (as VirtualBox's and Parallels's), so it carries spaces and the ` - ` and is taken whole as the
/// line's remainder after the two leading columns, never split on its own spaces.
pub(crate) fn utm_running_vm_names(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let (uuid, rest) = split_lead(line)?;
            // The header row (and any line without the two leading columns) is not a VM.
            if uuid.eq_ignore_ascii_case("uuid") {
                return None;
            }
            let (status, name) = split_lead(rest)?;
            let name = name.trim();
            (status.eq_ignore_ascii_case("started") && !name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// The first whitespace-delimited token of `s` and the rest after it, each with its own leading
/// whitespace trimmed: reads `utmctl`'s space-padded columns without collapsing a VM name's spaces.
fn split_lead(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace)?;
    Some((&s[..end], s[end..].trim_start()))
}

/// Lab ids from `vmrun list` (one running `.vmx` path per line after a count header): a lab's
/// VM lives under `.../labs/<id>/...`.
pub(crate) fn lab_ids_from_vmrun(out: &str) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for line in out.lines() {
        let line = line.trim();
        if let Some(id) = id_after_labs(line) {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

/// The `<id>` in a path like `/Users/x/labs/<id>/.isoloom/vagrant/...`: the `labs/` folder
/// followed by a lab's own layout (a parent folder named `labs`, say an account, comes first
/// and is not it).
fn id_after_labs(path: &str) -> Option<String> {
    // `vmrun list` prints Windows paths with backslashes.
    let path = path.replace('\\', "/");
    let segments: Vec<&str> = path.split('/').collect();
    let under_labs = |i: usize| segments.get(i) == Some(&"labs") && segments.get(i + 1).is_some_and(|id| is_lab_id(id));
    // A lab VM's own layout (`.isoloom/...`).
    let lab_layout = |i: usize| segments.get(i + 2).is_some_and(|d| d.starts_with(super::isothrum::OUTPUT_DIR));
    let at = (0..segments.len()).rev().find(|&i| under_labs(i) && lab_layout(i)).or_else(|| (0..segments.len()).rev().find(|&i| under_labs(i)))?;
    Some(segments[at + 1].to_string())
}

/// The ids of labs running on this machine right now, asked of Docker and of each hypervisor.
/// Best effort: an engine that is absent or errors simply contributes nothing. Read-only and
/// timed (never hangs the caller), so it is safe to poll.
pub async fn running_lab_ids(dirs: &AppDirs) -> Vec<String> {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    // One scan at a time: the UI polls every few seconds, and a wedged hypervisor makes each
    // probe wait out its timeout, so overlapping scans only stacked up stuck VBoxManage calls.
    // A poll during a scan gets the last answer.
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let Some(_flight) = crate::platform::sync::Busy::try_take(&IN_FLIGHT) else {
        return LAST.lock().map(|l| l.clone()).unwrap_or_default();
    };
    let ids = scan(dirs).await;
    if let Ok(mut last) = LAST.lock() {
        last.clone_from(&ids);
    }
    ids
}

async fn scan(dirs: &AppDirs) -> Vec<String> {
    let mut ids = BTreeSet::new();
    // Docker: the Compose project of every running container. A failure here is worth a warning
    // (the engine is missing or broken); an empty result is simply "nothing running".
    match run_read("docker", &["ps", "--filter", "label=com.docker.compose.project", "--format", "{{.Label \"com.docker.compose.project\"}}"], None).await {
        Ok(out) => ids.extend(lab_ids_from_docker_projects(&out)),
        Err(e) => log::warn!("running-labs scan: docker ps failed: {e}"),
    }
    // Hypervisors: their running VMs, named after the installed labs.
    let by_name = match (super::paths::labs_root(dirs), super::paths::runner_home(dirs)) {
        (Ok(labs), Ok(home)) => lab_stems(&labs, &home),
        _ => HashMap::new(),
    };
    ids.extend(vm_lab_ids(&by_name).await);
    let ids: Vec<String> = ids.into_iter().collect();
    log::debug!("running-labs scan: {} running lab(s) {:?}", ids.len(), ids);
    ids
}

/// The ids of labs whose VMs a hypervisor reports running. Each hypervisor may simply not be
/// installed, which contributes nothing; this is split off [`scan`] so neither grows unwieldy as
/// hypervisors are added. VMware reads the `.vmx` path, so it needs no names; the rest match the
/// installed labs' VM-name prefixes, so they are skipped when nothing is installed.
async fn vm_lab_ids(by_name: &HashMap<String, String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(out) = read_vms("VMware", "vmrun", &["list"]).await {
        ids.extend(lab_ids_from_vmrun(&out));
    }
    if by_name.is_empty() {
        return ids;
    }
    // The two name-based families, each its own helper so this stays small as hypervisors are
    // added: the ` - ` label (VirtualBox, Parallels, UTM) and the `<sep>` prefix (libvirt, Hyper-V).
    ids.extend(middot_vm_ids(by_name).await);
    ids.extend(prefixed_vm_ids(by_name).await);
    ids
}

/// Lab ids from the hypervisors that name a VM `<stem> - <machine>`: VirtualBox, Parallels and UTM.
/// Each may be absent, which contributes nothing. `by_name` is assumed non-empty (the caller skips
/// these when no lab is installed, since the match is by the installed labs' names).
async fn middot_vm_ids(by_name: &HashMap<String, String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Some(out) = read_vms("VirtualBox", "VBoxManage", &["list", "runningvms"]).await {
        let names = vbox_vm_names(&out);
        ids.extend(lab_ids_from_vm_names(names.iter().map(String::as_str), by_name));
    }
    if let Some(out) = read_vms("Parallels", "prlctl", &["list", "--running", "--no-header", "-o", "name"]).await {
        ids.extend(lab_ids_from_vm_names(out.lines().map(str::trim), by_name));
    }
    // UTM (macOS): a started VM carries the same `<stem> - <machine>` label. Only ask utmctl when
    // UTM is already running: `utmctl list` launches UTM.app otherwise, popping the app open on
    // every running-labs scan. `pgrep -x UTM` is a read-only check that cannot launch it (and on
    // Linux/Windows it finds nothing, so utmctl is skipped there too).
    if run_read("pgrep", &["-x", "UTM"], None).await.is_ok()
        && let Some(out) = read_vms("UTM", "utmctl", &["list"]).await
    {
        let names = utm_running_vm_names(&out);
        ids.extend(lab_ids_from_vm_names(names.iter().map(String::as_str), by_name));
    }
    ids
}

/// Lab ids from the hypervisors that name a VM `<lab><sep><machine>`: libvirt (`_`) and Hyper-V
/// (`-`). Each may be absent, which contributes nothing. `by_name` is assumed non-empty.
async fn prefixed_vm_ids(by_name: &HashMap<String, String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    // libvirt/KVM, and QEMU through libvirt: a running domain is `<stem>_<machine>` (vagrant-libvirt's
    // `default_prefix`). Ask both the default URI (a user session, or `$LIBVIRT_DEFAULT_URI`) and
    // the system instance, since vagrant-libvirt deploys to `qemu:///system` by default, which a
    // plain user `virsh` does not read.
    for (name, args) in [
        ("libvirt (default URI)", ["list", "--name", "--state-running"].as_slice()),
        ("libvirt (qemu:///system)", ["-c", "qemu:///system", "list", "--name", "--state-running"].as_slice()),
    ] {
        if let Some(out) = read_vms(name, "virsh", args).await {
            ids.extend(lab_ids_from_prefixed_vm_names(out.lines(), by_name, '_'));
        }
    }
    // Hyper-V (Windows only; elsewhere the command isn't there and the probe just contributes
    // nothing): a VM is `<stem>-<machine>`. The running ones by name. A
    // Windows without Hyper-V's module lists nothing, rather than failing every scan.
    if let Some(out) = read_vms("Hyper-V", "powershell", &["-NoProfile", "-Command", HYPERV_RUNNING]).await {
        ids.extend(lab_ids_from_prefixed_vm_names(out.lines(), by_name, '-'));
    }
    ids
}

/// Hyper-V's running VMs by name; nothing where its PowerShell module isn't installed.
const HYPERV_RUNNING: &str =
    "if (Get-Command Get-VM -ErrorAction SilentlyContinue) { Get-VM | Where-Object { $_.State -eq 'Running' } | ForEach-Object { $_.Name } }";

/// What a hypervisor's `program args` lists of its running VMs, `name` saying which in the log.
/// Not installed: nothing of it to find, said nowhere. Past the app's time limit: said as not
/// responding. Any other failure (a broken install, a service down, a right missing) hides its
/// labs from the scan, so it is said too. The one rule of every probe of the scan.
async fn read_vms(name: &str, program: &'static str, args: &[&str]) -> Option<String> {
    match run_read(program, args, None).await {
        Ok(out) => Some(out),
        Err(e) if crate::platform::exec::timed_out(&e) => {
            log::warn!("running-labs scan: {name} isn't responding ({e})");
            None
        }
        Err(crate::error::Error::ToolMissing { .. }) => None,
        Err(e) => {
            log::warn!("running-labs scan: {name} couldn't list its running VMs ({e})");
            None
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
