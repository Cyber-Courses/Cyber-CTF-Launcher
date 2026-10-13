//! Clearing what a crashed or interrupted VM lab left behind: Vagrant state recorded on another
//! provider, and orphaned VirtualBox/Parallels VMs and folders.

use super::*;

/// Reconciles the lab dir against the provider it is about to start on. Vagrant records a
/// machine's provider under `.vagrant/machines/<name>/<provider>/`; starting on a different one
/// (e.g. the lab last ran locally on VirtualBox and is now sent to an ESXi host) makes
/// `vagrant up` refuse with "an active machine was found with a different provider". When the
/// recorded provider differs, tear the old one's machines down (best effort, using whatever env
/// it needs) and reset `.vagrant` so the new provider starts from a clean slate.
pub async fn reconcile_provider(dir: &Path, requested: Provider, env: &[(String, String)], log: &mut impl FnMut(String)) {
    if !recorded_on_other_provider(dir, requested) {
        return;
    }
    log("This lab last ran on a different target. Clearing that state so it can start fresh here…".into());
    stop_bounded(dir, env).await;
    let _ = std::fs::remove_dir_all(dir.join(".vagrant"));
}

/// Whether Vagrant still tracks a created machine of this folder (on any provider): it writes
/// `.vagrant/machines/<name>/<provider>/id` when the machine is created and removes it when it
/// is destroyed. Read from the files only, so it never reaches the host.
pub(in crate::runtime) fn has_machines(dir: &Path) -> bool {
    let Ok(machines) = std::fs::read_dir(dir.join(".vagrant").join("machines")) else { return false };
    machines.flatten().any(|m| std::fs::read_dir(m.path()).is_ok_and(|provs| provs.flatten().any(|p| p.path().join("id").is_file())))
}

/// Whether Vagrant recorded a machine of this folder under a provider other than `requested`.
pub(super) fn recorded_on_other_provider(dir: &Path, requested: Provider) -> bool {
    let machines = dir.join(".vagrant").join("machines");
    let Ok(entries) = std::fs::read_dir(&machines) else { return false };
    let mut mismatch = false;
    for machine in entries.flatten() {
        if let Ok(provs) = std::fs::read_dir(machine.path()) {
            for prov in provs.flatten() {
                // Only a folder holding an `id` is a machine (as `has_machines` counts them): a
                // destroy or a plain `vagrant status` leaves empty provider folders behind.
                if prov.path().join("id").is_file() && prov.file_name().to_string_lossy() != requested.id() {
                    mismatch = true;
                }
            }
        }
    }
    mismatch
}

/// The on-disk folder VirtualBox creates for a VM is a sanitized form of its name: the middot in
/// "lab · dc01" becomes "lab - dc01". This returns the exact folder name candidates for a VM
/// name, so an orphaned folder is matched EXACTLY (never by lossy normalization, which could
/// collide with an unrelated VM like "lab-dc01").
pub(super) fn folder_candidates(name: &str) -> [String; 2] {
    [name.to_string(), name.replace('\u{00b7}', "-")]
}

/// The VM names Isoloom wrote into the Vagrantfile, used to scope orphan cleanup strictly to
/// this lab's own VMs. Every provider block names the VM on a `v.name` / `v.guest_name` /
/// `v.vmx["displayName"]` line, as the quoted string after the `=` (for `displayName` the key
/// itself is quoted, so taking the value after `=` is what's correct, not the first quote).
pub(in crate::runtime) fn vm_names(dir: &Path) -> Vec<String> {
    vm_names_from(&std::fs::read_to_string(dir.join("Vagrantfile")).unwrap_or_default())
}

pub(in crate::runtime) fn vm_names_from(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for raw in text.lines() {
        let l = raw.trim();
        let is_name_line = l.starts_with("v.name") || l.starts_with("v.guest_name") || l.contains("displayName\"]");
        let Some(eq) = l.find('=') else { continue };
        if !is_name_line {
            continue;
        }
        let after = &l[eq + 1..];
        if let Some(open) = after.find('"')
            && let Some(len) = after[open + 1..].find('"')
        {
            let name = &after[open + 1..open + 1 + len];
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// VirtualBox's default machine folder (where VM directories live), parsed from
/// `VBoxManage list systemproperties`; falls back to `~/VirtualBox VMs`.
async fn vbox_machine_folder() -> Option<PathBuf> {
    if let Ok(out) = run("VBoxManage", &["list", "systemproperties"], None).await
        && let Some(path) = parse_machine_folder(&out)
    {
        return Some(path);
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("VirtualBox VMs"))
}

/// The `Default machine folder:` of `VBoxManage list systemproperties`.
pub(super) fn parse_machine_folder(out: &str) -> Option<PathBuf> {
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("Default machine folder:") {
            let path = path.trim();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

/// `(name, uuid)` of each VM in `VBoxManage list vms` (`"<name>" {<uuid>}` per line).
pub(super) fn parse_vbox_vms(out: &str) -> Vec<(String, String)> {
    let mut vms = Vec::new();
    for line in out.lines() {
        let (Some(open), Some(close)) = (line.rfind('{'), line.rfind('}')) else { continue };
        if close <= open {
            continue;
        }
        let uuid = line[open + 1..close].to_string();
        let name = line[..open].trim().trim_matches('"').to_string();
        vms.push((name, uuid));
    }
    vms
}

/// Deletes the folders under `base` named EXACTLY like one of `wanted`'s VirtualBox folders.
pub(super) fn remove_leftover_folders(base: &Path, wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
    let folders: HashSet<String> = wanted.iter().flat_map(|n| folder_candidates(n)).collect();
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && folders.contains(entry.file_name().to_string_lossy().as_ref()) {
            log(format!("Removing a leftover VM folder: {}", entry.file_name().to_string_lossy()));
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// The names in `prlctl list -o name` output that are ours.
pub(super) fn ours_in_prlctl<'a>(out: &'a str, wanted: &HashSet<String>) -> Vec<&'a str> {
    out.lines().map(str::trim).filter(|name| !name.is_empty() && wanted.contains(*name)).collect()
}

/// Removes VirtualBox VMs and leftover machine folders that belong to this lab (name matches one
/// of `wanted` EXACTLY) but that `vagrant destroy` can't reach: a VM a failed import half-created
/// and never registered with Vagrant, or an orphaned folder that makes the next import fail with
/// VERR_ALREADY_EXISTS. Exact-match only — a lossy match could delete an unrelated VM (e.g. a
/// user's own "lab-dc01" vs this lab's "lab · dc01"), which `unregistervm --delete` can't undo.
pub(in crate::runtime) async fn recover_virtualbox(wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
    // Registered VMs of ours that are still around: power off and delete (removes their files).
    if let Ok(out) = run("VBoxManage", &["list", "vms"], None).await {
        for (name, uuid) in parse_vbox_vms(&out) {
            if wanted.contains(&name) {
                log(format!("Removing a leftover VM left by a previous run: {name}"));
                // A VM in the "saved" state (suspended, or the host slept) can't be powered off
                // or unregistered until its saved state is discarded; on any other state this
                // is a harmless error.
                let _ = run("VBoxManage", &["discardstate", &uuid], None).await;
                let _ = run("VBoxManage", &["controlvm", &uuid, "poweroff"], None).await;
                let _ = run("VBoxManage", &["unregistervm", &uuid, "--delete"], None).await;
            }
        }
    }
    // Unregistered leftover folders (the import failed before registering the VM): delete only a
    // folder whose name is EXACTLY one of our VMs' sanitized folder names.
    if let Some(base) = vbox_machine_folder().await {
        remove_leftover_folders(&base, wanted, log);
    }
}

/// Parallels equivalent of `recover_virtualbox` (best effort, exact name match): `prlctl list`
/// then delete ours.
pub(in crate::runtime) async fn recover_parallels(wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
    let Ok(out) = run("prlctl", &["list", "-a", "--no-header", "-o", "name"], None).await else { return };
    for name in ours_in_prlctl(&out, wanted) {
        log(format!("Removing a leftover VM left by a previous run: {name}"));
        let _ = run("prlctl", &["stop", name, "--kill"], None).await;
        let _ = run("prlctl", &["delete", name], None).await;
    }
}

/// Clears leftover local-VM state from a crashed or interrupted run that `vagrant destroy`
/// alone can't fix: a normal destroy first (tracked VMs and most locks), then hypervisor-level
/// removal of this lab's orphaned VMs and folders, then a reset of `.vagrant` so a stale lock a
/// killed process left behind can't block the next `vagrant up`. Best effort throughout: every
/// step tolerates "nothing there". Scoped to this lab's own VM names, so it never touches
/// unrelated VMs on the machine.
pub async fn recover_local(dir: &Path, provider: Provider, log: &mut impl FnMut(String)) {
    stop_bounded(dir, &[]).await;
    let names: HashSet<String> = vm_names(dir).into_iter().collect();
    if !names.is_empty() {
        match provider {
            Provider::Virtualbox => recover_virtualbox(&names, log).await,
            Provider::Parallels => recover_parallels(&names, log).await,
            // VMware/UTM/QEMU/libvirt: the destroy above plus the .vagrant reset below are the
            // best we can do portably; their orphan cleanup can be added if it shows up in use.
            _ => {}
        }
    }
    let _ = std::fs::remove_dir_all(dir.join(".vagrant"));
}
