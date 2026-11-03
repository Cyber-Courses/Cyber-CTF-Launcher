//! Finding labs that are actually running on this machine, by asking the engines themselves:
//! Docker for its running containers, and the hypervisors (VirtualBox, Parallels, VMware, UTM,
//! libvirt and Hyper-V) for their running VMs. Never a cache or a marker: Vagrant's global index,
//! for one, keeps saying "running" long after the VMs are gone. Each lab is deployed under a
//! predictable name, a Compose project `cyberctf-<id>` or VMs named for the lab (`<lab> · <machine>` on
//! VirtualBox/VMware/Parallels/UTM, `<lab>_<machine>` on libvirt, `<lab>-<machine>` on Hyper-V),
//! so one scan recovers which labs are up even after a crash or restart, when the in-memory
//! "deploying" set and the status cache are gone.
//!
//! Not covered: VMs that vagrant-qemu runs directly, with no libvirt and no registry to list them
//! (a bare `qemu-system` process per VM). They would have to be found by scanning the process table
//! for a `qemu-system` whose disk path is under `labs/<id>/.isoloom/`, which the engine's own source
//! does not guarantee (the vagrant-qemu gem spawns it), so it is left out rather than guessed at.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::platform::AppDirs;
use crate::platform::exec::run_read;

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
        let v = l.strip_prefix("name:")?.trim();
        // As the YAML parser (and so the VM names) reads it: a quoted value whole, a bare one up
        // to a comment.
        let v = if let Some(q @ ('"' | '\'')) = v.chars().next() {
            v[1..].split(q).next().unwrap_or_default()
        } else {
            let cut = [" #", "\t#"].iter().filter_map(|m| v.find(m)).min().unwrap_or(v.len());
            v[..cut].trim()
        };
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

/// The lab ids of running VMs named `<lab><sep><machine>`, for the labs in `by_name`: libvirt
/// domains (`<lab>_<machine>`, from vagrant-libvirt's `default_prefix`) and Hyper-V/ESXi VMs
/// (`<lab>-<machine>`). Unlike the ` · ` separator, `_` and `-` can occur inside a lab's own name,
/// so a lab is matched as a prefix, not by splitting: the longest installed name that leaves a
/// non-empty machine wins. As with ` · attacker`, a lone attacker VM does not mean the lab is up.
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
        // On that most-specific reading, a lone attacker VM (`<lab><sep>attacker`) runs beside the
        // lab and does not mean it is up: so `goad-light-attacker` counts for neither lab.
        if let Some((_, id, machine)) = best
            && machine != "attacker"
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

/// Names of the started VMs from `utmctl list` (`<uuid>  <status>  <name>`, under a `UUID Status
/// Name` header). Only `started` VMs count. A UTM VM's name is the lab's `<lab> · <machine>` label
/// (as VirtualBox's and Parallels's), so it carries spaces and the ` · ` and is taken whole as the
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
        // The attack VM (under `labs/<id>/.cyberctf-attack-vm/`) runs beside the lab; on its own it
        // does not mean the lab's targets are up, so — as the VM-name scan does for `· attacker` —
        // it is not counted.
        if line.replace('\\', "/").contains("/.cyberctf-attack-vm/") {
            continue;
        }
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
    // A target VM's own layout (`.isoloom/...`). The attack VM's `.cyberctf-attack-vm/...` is
    // filtered out before here, so it is not a layout a lab-is-running path can match on.
    let lab_layout = |i: usize| segments.get(i + 2).is_some_and(|d| d.starts_with(".isoloom"));
    let at = (0..segments.len()).rev().find(|&i| under_labs(i) && lab_layout(i)).or_else(|| (0..segments.len()).rev().find(|&i| under_labs(i)))?;
    Some(segments[at + 1].to_string())
}

/// A plausible lab id: non-empty, bounded, and only the characters an id is built from. Keeps a
/// stray token (or an attacker project like `cyberctf-<id>-attack`, which is not an id) out.
pub(crate) fn is_lab_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The ids of labs running on this machine right now, asked of Docker and of each hypervisor.
/// Best effort: an engine that is absent or errors simply contributes nothing. Read-only and
/// timed (never hangs the caller), so it is safe to poll.
pub async fn running_lab_ids(dirs: &AppDirs) -> Vec<String> {
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
    let _done = crate::platform::ResetOnDrop(&IN_FLIGHT);
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
    let by_name = dirs.data().map(|d| lab_names(&d.join("labs"))).unwrap_or_default();
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
    if let Ok(out) = run_read("vmrun", &["list"], None).await {
        ids.extend(lab_ids_from_vmrun(&out));
    }
    if by_name.is_empty() {
        return ids;
    }
    // The two name-based families, each its own helper so this stays small as hypervisors are
    // added: the ` · ` label (VirtualBox, Parallels, UTM) and the `<sep>` prefix (libvirt, Hyper-V).
    ids.extend(middot_vm_ids(by_name).await);
    ids.extend(prefixed_vm_ids(by_name).await);
    ids
}

/// Lab ids from the hypervisors that name a VM `<lab> · <machine>`: VirtualBox, Parallels and UTM.
/// Each may be absent, which contributes nothing. `by_name` is assumed non-empty (the caller skips
/// these when no lab is installed, since the match is by the installed labs' names).
async fn middot_vm_ids(by_name: &HashMap<String, String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    match run_read("VBoxManage", &["list", "runningvms"], None).await {
        Ok(out) => {
            let names = vbox_vm_names(&out);
            ids.extend(lab_ids_from_vm_names(names.iter().map(String::as_str), by_name));
        }
        Err(e) if e.to_string().contains("timed out") => log::warn!("running-labs scan: VirtualBox isn't responding ({e})"),
        Err(_) => {}
    }
    if let Ok(out) = run_read("prlctl", &["list", "--running", "--no-header", "-o", "name"], None).await {
        ids.extend(lab_ids_from_vm_names(out.lines().map(str::trim), by_name));
    }
    // UTM (macOS): a started VM carries the same `<lab> · <machine>` label. Only ask utmctl when
    // UTM is already running: `utmctl list` launches UTM.app otherwise, popping the app open on
    // every running-labs scan. `pgrep -x UTM` is a read-only check that cannot launch it (and on
    // Linux/Windows it finds nothing, so utmctl is skipped there too).
    if run_read("pgrep", &["-x", "UTM"], None).await.is_ok()
        && let Ok(out) = run_read("utmctl", &["list"], None).await
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
    // libvirt/KVM, and QEMU through libvirt: a running domain is `<lab>_<machine>` (vagrant-libvirt's
    // `default_prefix`). Ask both the default URI (a user session, or `$LIBVIRT_DEFAULT_URI`) and
    // the system instance, since vagrant-libvirt deploys to `qemu:///system` by default, which a
    // plain user `virsh` does not read.
    for args in [["list", "--name", "--state-running"].as_slice(), ["-c", "qemu:///system", "list", "--name", "--state-running"].as_slice()] {
        if let Ok(out) = run_read("virsh", args, None).await {
            ids.extend(lab_ids_from_prefixed_vm_names(out.lines(), by_name, '_'));
        }
    }
    // Hyper-V (Windows only; elsewhere the command isn't there and the probe just contributes
    // nothing): a VM is `<lab>-<machine>` (Hyper-V allows no `·`). The running ones by name.
    if let Ok(out) =
        run_read("powershell", &["-NoProfile", "-Command", "Get-VM | Where-Object { $_.State -eq 'Running' } | ForEach-Object { $_.Name }"], None).await
    {
        ids.extend(lab_ids_from_prefixed_vm_names(out.lines(), by_name, '-'));
    }
    ids
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// The UTM probe end to end with a scripted `utmctl`, once `pgrep` reports UTM running: its
    /// started VMs reach the installed labs through the name reader and the ` · ` matcher. A
    /// `utmctl` that fails (UTM quit between the check and the call) contributes nothing, without
    /// failing the scan.
    #[tokio::test]
    async fn a_scripted_utmctl_maps_started_vms_to_labs_and_a_failing_one_contributes_nothing() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let by_name: HashMap<String, String> = [("minilab".to_string(), "mid".to_string())].into();
        let listing = "UUID  Status  Name\na started  minilab · dc01\nb started  minilab · ws01\nc stopped  minilab · router\n";
        let tools = Tools::new(move |program, _| match program {
            "pgrep" => ok("4242"),
            "utmctl" => ok(listing),
            _ => None,
        });
        let found = std::collections::BTreeSet::from(["mid".to_string()]);
        assert_eq!(super::middot_vm_ids(&by_name).await, found);
        assert_eq!(super::vm_lab_ids(&by_name).await, found);
        assert!(tools.called("utmctl list"), "{:?}", tools.calls());
        drop(tools);
        let tools = Tools::new(|program, _| match program {
            "pgrep" => ok("4242"),
            "utmctl" => fail("Error: UTM is not running"),
            _ => None,
        });
        assert!(super::middot_vm_ids(&by_name).await.is_empty());
        assert!(super::vm_lab_ids(&by_name).await.is_empty());
        assert!(tools.called("utmctl list"), "{:?}", tools.calls());
    }

    /// The fix for UTM popping open on every scan: when `pgrep` reports UTM is not running,
    /// `utmctl` is never called, since calling it would launch UTM.app.
    #[tokio::test]
    async fn utmctl_is_not_called_when_utm_is_not_running() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let by_name: HashMap<String, String> = [("minilab".to_string(), "mid".to_string())].into();
        let tools = Tools::new(|program, _| match program {
            "pgrep" => fail("no matching process"),
            // Scripted so that, were it ever reached, it would map a VM: the point is that it is not.
            "utmctl" => ok("UUID  Status  Name\na started  minilab · dc01\n"),
            _ => None,
        });
        assert!(super::middot_vm_ids(&by_name).await.is_empty());
        assert!(!tools.called("utmctl list"), "utmctl must not run when UTM is down: {:?}", tools.calls());
    }

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
    fn utmctl_lists_only_started_vms_keeping_each_name_whole() {
        // `utmctl list` columns are space-padded; a name carries its own spaces and the ` · `, so it
        // must come back whole (not collapsed, not cut at its first space). Only `started` counts.
        let out = "UUID                                 Status   Name\n\
                   11111111-1111-1111-1111-111111111111 started  minilab · dc01\n\
                   22222222-2222-2222-2222-222222222222 stopped  minilab · ws01\n\
                   33333333-3333-3333-3333-333333333333 paused   minilab · router\n\
                   44444444-4444-4444-4444-444444444444 started  other box\n";
        assert_eq!(super::utm_running_vm_names(out), vec!["minilab · dc01".to_string(), "other box".to_string()]);
        // No VMs, or only the header, is empty (not a panic).
        assert_eq!(super::utm_running_vm_names("UUID Status Name\n"), Vec::<String>::new());
        assert_eq!(super::utm_running_vm_names(""), Vec::<String>::new());
    }

    #[test]
    fn utm_started_vms_map_back_to_installed_labs_and_a_lone_attacker_does_not_count() {
        // The full UTM path: utmctl output, through the name reader, through the ` · ` matcher.
        let by_name: HashMap<String, String> = [("minilab".to_string(), "mid".to_string())].into();
        let out = "UUID  Status  Name\n\
                   a started  minilab · dc01\n\
                   b started  minilab · ws01\n\
                   c stopped  minilab · router\n\
                   d started  minilab · attacker\n\
                   e started  elsewhere · box\n";
        let names = super::utm_running_vm_names(out);
        assert_eq!(names, ["minilab · dc01", "minilab · ws01", "minilab · attacker", "elsewhere · box"]);
        // Two target VMs of the lab are up: the lab runs (once), the foreign VM and the attacker aside.
        assert_eq!(super::lab_ids_from_vm_names(names.iter().map(String::as_str), &by_name), vec!["mid".to_string()]);
        // Only its attacker started: not the lab running.
        let only_attacker = "UUID  Status  Name\nx started  minilab · attacker\n";
        let names = super::utm_running_vm_names(only_attacker);
        assert_eq!(super::lab_ids_from_vm_names(names.iter().map(String::as_str), &by_name), Vec::<String>::new());
    }

    #[test]
    fn vmrun_paths_name_the_lab_by_its_folder() {
        let out = "Total running VMs: 1\n/Users/x/Library/App/labs/goad-light/.isoloom/vagrant/.vagrant/machines/dc01/vmware_desktop/x.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(out), vec!["goad-light".to_string()]);
        assert_eq!(super::lab_ids_from_vmrun("Total running VMs: 0\n"), Vec::<String>::new());
    }

    #[test]
    fn a_running_attack_vm_alone_does_not_mean_the_vmware_lab_runs() {
        // The attack VM is under `labs/<id>/.cyberctf-attack-vm/`; like the VM-name scan's
        // `· attacker` exclusion, it is not counted on its own (it runs beside the lab).
        let attacker = "Total running VMs: 1\n/Users/x/labs/goad-light/.cyberctf-attack-vm/.vagrant/machines/attacker/vmware_desktop/x.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(attacker), Vec::<String>::new());
        // A target VM up alongside it: the lab runs, counted once from the target.
        let both = "Total running VMs: 2\n\
                    /Users/x/labs/goad-light/.cyberctf-attack-vm/.vagrant/machines/attacker/vmware_desktop/x.vmx\n\
                    /Users/x/labs/goad-light/.isoloom/vagrant/.vagrant/machines/dc01/vmware_desktop/y.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(both), vec!["goad-light".to_string()]);
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
    fn isoloom_name_takes_the_top_level_key_only() {
        // A commented-out or indented `name:` (a machine's, a tool's) is not the lab's.
        assert_eq!(super::isoloom_name("# name: old\nmachines:\n  name: web\nname: minilab\n").as_deref(), Some("minilab"));
        assert_eq!(super::isoloom_name("name:minilab\n").as_deref(), Some("minilab"), "no space after the colon");
        assert_eq!(super::isoloom_name("name: 'minilab'\n").as_deref(), Some("minilab"));
        assert_eq!(super::isoloom_name("name:   \n").as_deref(), None, "blank");
        assert_eq!(super::isoloom_name("namespace: x\nname_old: y\n"), None, "other keys starting with name");
        // Windows line endings and a trailing space are no part of the name.
        assert_eq!(super::isoloom_name("version: 1\r\nname: minilab \r\n").as_deref(), Some("minilab"));
    }

    #[test]
    fn a_comment_after_the_name_is_not_part_of_it() {
        // The VMs are named after `spec.name` as isoloom-spec parses it (YAML drops the comment);
        // the scan's line reader keeps it, and `lab_names` then maps "minilab # ..." to the lab,
        // which no VM name starts with: a VirtualBox/Parallels lab is never found running.
        // Fails with: left: Some("minilab # the lab's name"), right: Some("minilab").
        // Suggested fix: parse the line's value as YAML (`serde_yaml_ng::from_str::<String>`),
        // or cut at " #" before trimming the quotes.
        let yaml = "version: 1\nname: minilab # the lab's name\n";
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(parsed["name"].as_str(), Some("minilab"), "what the spec parser (and the VM names) say");
        assert_eq!(super::isoloom_name(yaml).as_deref(), Some("minilab"));
    }

    #[test]
    fn docker_project_ids_are_checked_at_the_id_boundaries() {
        let max = "a".repeat(64);
        let over = "a".repeat(65);
        let out = format!("cyberctf-{max}\ncyberctf-{over}\ncyberctf-\ncyberctf-é\ncyberctf-a b\ncyberctf-ok_id-1\r\n  cyberctf-spaced  \ncyberctf-x/../y\n");
        assert_eq!(super::lab_ids_from_docker_projects(&out), vec![max, "ok_id-1".to_string(), "spaced".to_string()]);
    }

    #[test]
    fn vbox_listings_skip_lines_that_are_not_a_quoted_name() {
        let out = "\"minilab-1 · dc01\" {0185}\r\ninaccessible {dead}\n\n\"\" {empty}\n  \"spaced · ws01\" {3eeb}\n\"unterminated {x}\n";
        assert_eq!(super::vbox_vm_names(out), ["minilab-1 · dc01", "", "spaced · ws01"]);
    }

    #[test]
    fn a_vm_named_only_attacker_or_without_the_separator_does_not_mean_a_lab_runs() {
        let by_name: HashMap<String, String> = [("minilab-1".to_string(), "goad".to_string())].into();
        let names = ["minilab-1 · attacker", "minilab-1", "minilab-1·dc01", "minilab-1 · ", "minilab-1 · attacker2", " · dc01"];
        // The attack VM alone: no. A machine named like the attacker but not exactly: yes.
        assert_eq!(super::lab_ids_from_vm_names(names.into_iter(), &by_name), ["goad".to_string()]);
        assert_eq!(super::lab_ids_from_vm_names(["minilab-1 · attacker"].into_iter(), &by_name), Vec::<String>::new());
        assert_eq!(super::lab_ids_from_vm_names(["minilab-1 · "].into_iter(), &by_name), ["goad".to_string()], "an empty machine name still names the lab");
    }

    #[test]
    fn libvirt_domains_name_their_lab_by_the_underscore_prefix() {
        // vagrant-libvirt names a running domain `<lab>_<machine>` (its default_prefix). The lab is
        // matched as a prefix, so a name carrying its own `-` reads whole.
        let by_name: HashMap<String, String> = [("minilab".to_string(), "id1".to_string()), ("goad-light".to_string(), "id2".to_string())].into();
        let domains = ["minilab_dc01", "goad-light_dc01", "goad-light_ws01", "unknown_x"];
        // Two machines of goad-light collapse to one id; an unknown domain is ignored.
        assert_eq!(super::lab_ids_from_prefixed_vm_names(domains.into_iter(), &by_name, '_'), vec!["id1".to_string(), "id2".to_string()]);
        // The attacker domain on its own does not mean the lab runs.
        assert_eq!(super::lab_ids_from_prefixed_vm_names(["minilab_attacker"].into_iter(), &by_name, '_'), Vec::<String>::new());
    }

    #[test]
    fn hyperv_vms_name_their_lab_by_the_dash_prefix_longest_first() {
        // Hyper-V allows no `·`, so a VM is `<lab>-<machine>`; the `-` also joins a kebab lab name,
        // so the longest installed name that leaves a machine wins.
        let by_name: HashMap<String, String> = [("goad".to_string(), "g".to_string()), ("goad-light".to_string(), "gl".to_string())].into();
        assert_eq!(super::lab_ids_from_prefixed_vm_names(["goad-light-dc01"].into_iter(), &by_name, '-'), vec!["gl".to_string()]);
        assert_eq!(super::lab_ids_from_prefixed_vm_names(["goad-ws01"].into_iter(), &by_name, '-'), vec!["g".to_string()]);
        // goad-light's attacker, read most-specifically, is goad-light's attacker: it counts for
        // neither goad-light nor (as `light-attacker`) goad.
        assert_eq!(super::lab_ids_from_prefixed_vm_names(["goad-light-attacker"].into_iter(), &by_name, '-'), Vec::<String>::new());
        // A bare lab name (no machine) and a foreign VM do not count.
        assert_eq!(super::lab_ids_from_prefixed_vm_names(["other-vm", "goad"].into_iter(), &by_name, '-'), Vec::<String>::new());
    }

    #[test]
    fn vmrun_reads_windows_paths_and_skips_odd_lines() {
        let out = "Total running VMs: 2\r\nC:\\Users\\me\\AppData\\Roaming\\com.cyberctf\\labs\\goad-light\\.isoloom\\vagrant\\.vagrant\\machines\\dc01\\vmware_desktop\\x.vmx\r\n/Users/me/labs/\n/Users/me/labs/bad id/x.vmx\n/Users/me/other/x.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(out), vec!["goad-light".to_string()]);
    }

    #[test]
    fn a_labs_folder_earlier_in_the_path_does_not_mislead_the_vmrun_scan() {
        // The lab's id follows the launcher's own `labs/` folder, which sits under the app data
        // folder; a parent folder also named `labs` (an account `labs`, a `~/labs` checkout holding
        // the app data in a portable setup) comes first and its next segment is taken as the id.
        // Fails with: left: ["Library"], right: ["goad-light"].
        // Suggested fix: match the path against the app's own labs folder (`<app data>/labs/`),
        // or take the id from the `/labs/<id>/.isoloom` (or `/.cyberctf-attack-vm`) shape.
        let out = "Total running VMs: 1\n/Users/labs/Library/Application Support/com.cyberctf/labs/goad-light/.isoloom/vagrant/.vagrant/machines/dc01/vmware_desktop/x.vmx\n";
        assert_eq!(super::lab_ids_from_vmrun(out), vec!["goad-light".to_string()]);
    }

    #[test]
    fn an_unreadable_or_duplicate_name_leaves_the_lab_out_of_the_scan() {
        let labs = std::env::temp_dir().join(format!("cyberctf-discover-dup-{}", rand::random::<u32>()));
        // Three labs: two with the same instance name (never guessed), one with a marker that
        // isn't an instance (plain name), and a folder that is a file.
        for (id, yaml, marker) in [("a", "name: twin\n", Some("1")), ("b", "name: twin\n", Some("1")), ("c", "name: solo\n", Some("0"))] {
            let d = labs.join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("isoloom.yml"), yaml).unwrap();
            if let Some(m) = marker {
                std::fs::write(d.join(crate::runtime::lab::INSTANCE_MARKER), m).unwrap();
            }
        }
        std::fs::write(labs.join("d"), "a file, not a lab").unwrap();
        let by_name = super::lab_names(&labs);
        assert_eq!(by_name.len(), 1, "{by_name:?}");
        assert_eq!(by_name.get("solo").map(String::as_str), Some("c"), "a marker of 0 is no instance: the plain name");
        std::fs::remove_dir_all(labs).unwrap();
    }
}

#[cfg(test)]
mod proptests;
