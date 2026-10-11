use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::model::Park;
use super::providers::Provider;
use super::{LabStatus, Machine};
use crate::error::Result;
use crate::exec::{run, run_env_timed, run_read, stream};

/// A status read must never hang the status poll: a wedged VirtualBox (its global lock held by
/// a stuck VBoxManage) would otherwise pile up one blocked `vagrant status` per poll tick.
const STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// How long a best-effort cleanup `vagrant destroy` may run before we give up waiting and fall
/// back to hypervisor-level teardown: a wedged VirtualBox (a stuck VBoxManage) must not hang a
/// start indefinitely.
const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// A cleanup `vagrant destroy`, bounded so it can't hang the caller. Errors and timeouts are
/// ignored: the hypervisor-level cleanup that follows handles whatever the destroy didn't.
async fn stop_bounded(dir: &Path, env: &[(String, String)]) {
    let _ = tokio::time::timeout(CLEANUP_TIMEOUT, stop(dir, env, |_l: String| {})).await;
}

/// Isoloom's provisioning controller: the machine that runs Ansible against the others. Not a
/// target, so it is powered off once the lab is built, and booted again to provision or check.
pub const CONTROLLER: &str = "isoloom-controller";

fn has_controller(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Vagrantfile")).is_ok_and(|t| t.contains(&format!("config.vm.define \"{CONTROLLER}\"")))
}

/// Whether this folder's VMs run on QEMU (vagrant-qemu). Its private networks link exactly two
/// VMs, so there is no room for an attack VM: the controller, on the lab network already, stays
/// on as the player's attack position.
pub fn on_qemu(dir: &Path) -> bool {
    std::fs::read_dir(dir.join(".vagrant").join("machines")).is_ok_and(|it| it.flatten().any(|m| m.path().join(Provider::Qemu.id()).is_dir()))
}

/// Powers the controller off (state kept) once the lab is built: it has no part in the attack
/// surface and would only cost memory. Best effort. On QEMU it stays on: it is the attacker.
async fn park_controller(dir: &Path, env: &[(String, String)], log: &mut impl FnMut(String)) {
    if !has_controller(dir) || on_qemu(dir) {
        return;
    }
    log("Powering off the setup machine (it comes back for provisioning and checks)…".into());
    if let Err(e) = vagrant(dir, &["halt", CONTROLLER], env, &mut *log).await {
        log(format!("The setup machine stays on: {e}"));
    }
}

/// A VirtualBox VM whose saved state was cut off (a suspend killed midway, a host crash) is
/// left `aborted-saved`, and VirtualBox then refuses every change to it ("the machine is not
/// mutable"), which makes `vagrant up` fail on this folder's VMs for good. The saved state is
/// worthless at that point: discard it, so the VM boots fresh from its disk. Best effort.
pub(super) async fn discard_aborted_saved(dir: &Path, log: &mut impl FnMut(String)) {
    for name in vm_names(dir) {
        let Ok(info) = run_read("VBoxManage", &["showvminfo", &name, "--machinereadable"], None).await else { continue };
        if info.lines().any(|l| l.trim() == "VMState=\"aborted-saved\"") {
            log(format!("{name}: its saved state is unusable (it was cut off); discarding it so the VM boots fresh."));
            let _ = run("VBoxManage", &["discardstate", &name], None).await;
        }
    }
}

/// `env` reaches the Vagrantfile and its provisioners (e.g. the evidence claim).
pub async fn start(dir: &Path, provider: Provider, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    discard_aborted_saved(dir, &mut log).await;
    vagrant(dir, &["up", "--provider", provider.id()], env, &mut log).await?;
    park_controller(dir, env, &mut log).await;
    Ok(())
}

/// Runs the lab's setup again on its running machines, without rebuilding it: the repair for a
/// machine whose setup was cut short (a domain join that timed out). With a controller, the
/// setup is its Ansible play against every machine, so the controller is booted, the play rerun
/// (after `machine`'s own steps, when one is named) and the controller powered off again.
/// Without one, it is each machine's own provisioners (`vagrant provision [machine]`).
pub async fn provision(dir: &Path, machine: Option<&str>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    if has_controller(dir) {
        discard_aborted_saved(dir, &mut log).await;
        log("Booting the setup machine…".into());
        vagrant(dir, &["up", CONTROLLER, "--no-provision"], env, &mut log).await?;
        if let Some(m) = machine.filter(|m| *m != CONTROLLER) {
            vagrant(dir, &["provision", m], env, &mut log).await?;
        }
        log("Running the lab's setup play…".into());
        let result = vagrant(dir, &["provision", CONTROLLER, "--provision-with", "ansible"], env, &mut log).await;
        park_controller(dir, env, &mut log).await;
        return result;
    }
    let mut args = vec!["provision"];
    if let Some(m) = machine {
        args.push(m);
    }
    vagrant(dir, &args, env, &mut log).await
}

/// A streamed Vagrant command whose failure leads with Ansible's own verdict.
async fn vagrant(dir: &Path, args: &[&str], env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    // When a provisioner's Ansible run fails, Vagrant only says "the SSH command responded with a
    // non-zero exit status"; the reason (`fatal: [ws01]: UNREACHABLE! ... winrm ... timed out`)
    // is hundreds of lines up in the stream. Keep Ansible's last verdict and lead the error with it.
    let mut verdict: Option<String> = None;
    let result = stream("vagrant", args, Some(dir), env, |line: String| {
        if let Some(v) = ansible_verdict(&line) {
            verdict = Some(v);
        }
        log(line);
    })
    .await;
    lead_with_verdict(result, verdict)
}

/// A failed command's error, led by Ansible's verdict when the stream had one.
fn lead_with_verdict(result: Result<()>, verdict: Option<String>) -> Result<()> {
    match (result, verdict) {
        (Err(crate::error::Error::CommandFailed { command, stderr }), Some(v)) => {
            Err(crate::error::Error::CommandFailed { command, stderr: format!("{v}\n{stderr}") })
        }
        (result, _) => result,
    }
}

/// Parks the lab's VMs without destroying them: `Pause` saves their state to disk (`vagrant
/// suspend`, resumes in seconds), `Shutdown` powers them off cleanly (`vagrant halt`).
pub async fn park(dir: &Path, mode: Park, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    stream("vagrant", &[park_verb(mode)], Some(dir), env, &mut log).await
}

/// The Vagrant command that parks VMs in `mode`.
fn park_verb(mode: Park) -> &'static str {
    match mode {
        Park::Pause => "suspend",
        Park::Shutdown => "halt",
    }
}

/// Brings parked VMs back: a saved VM resumes where it was, a powered-off one boots. Never
/// provisions again (the lab was built already; a second Ansible run could change it), and
/// stays on the provider the VMs were created with.
pub async fn resume(dir: &Path, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    discard_aborted_saved(dir, &mut log).await;
    let provider = status(dir, env).await.ok().and_then(|s| s.provider);
    stream("vagrant", &resume_args(provider.as_deref()), Some(dir), env, &mut log).await?;
    park_controller(dir, env, &mut log).await;
    Ok(())
}

/// `vagrant up` that brings parked VMs back on the provider they were created with.
fn resume_args(provider: Option<&str>) -> Vec<&str> {
    let mut args = vec!["up", "--no-provision"];
    if let Some(p) = provider {
        args.extend(["--provider", p]);
    }
    args
}

/// Ansible's own account of a failure in a streamed Vagrant line (the `<machine>: ` prefix
/// stripped): a `fatal: [host]: UNREACHABLE!/FAILED!` line, trimmed to a readable length.
fn ansible_verdict(line: &str) -> Option<String> {
    let body = line.trim();
    // Vagrant prefixes guest output with "<machine>: "; a bare line has no prefix to strip.
    let verdict = if body.starts_with("fatal: [") {
        body
    } else {
        let (machine, rest) = body.split_once(": ")?;
        if machine.contains(' ') || !rest.starts_with("fatal: [") {
            return None;
        }
        rest
    };
    let mut v = verdict.to_string();
    if v.len() > 300 {
        // Cut on a character boundary: the message may carry non-ASCII text.
        let cut = (0..=297).rev().find(|&i| v.is_char_boundary(i)).unwrap_or(0);
        v.truncate(cut);
        v.push_str("...");
    }
    Some(v)
}

/// Destroys the VMs so the next start restores the lab's initial state. `env` must carry
/// the same server connection as `start`: Vagrant re-evaluates the Vagrantfile.
pub async fn stop(dir: &Path, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let destroyed = stream("vagrant", &["destroy", "--force"], Some(dir), env, &mut log).await;
    // Vagrant only destroys what it still tracks. A lab re-installed from its registered commit
    // loses `.vagrant/` (Vagrant's machine index), so the VMs of an earlier run become orphans
    // Vagrant reports as `not_created` and leaves behind — "Stop & clean up" then looked like it
    // did nothing. Finish at the hypervisor, scoped to this lab's exact VM names, for every
    // hypervisor we know how to clean (each is a no-op where the names don't exist).
    let names: HashSet<String> = vm_names(dir).into_iter().collect();
    if !names.is_empty() {
        recover_virtualbox(&names, &mut log).await;
        recover_parallels(&names, &mut log).await;
    }
    destroyed
}

/// A `vagrant up` line that means the start tripped over leftover state from a crashed or
/// interrupted previous run (an orphaned hypervisor VM or its folder, a stale Vagrant lock)
/// rather than a genuine provisioning failure. Such a start can't succeed until the leftovers
/// are cleared, so the caller clears them (`recover_local`) and tries once more.
pub fn is_stale_state_error(line: &str) -> bool {
    // Only hypervisor/Vagrant *startup* errors that a cleanup+retry can actually fix. Deliberately
    // NOT the bare "already exists" / "already registered" / "name is already in use", which also
    // appear in ordinary successful provisioner output ("group docker already exists", a user or
    // network that already exists): matching those would destroy and re-provision a healthy VM.
    let l = line.to_ascii_lowercase();
    l.contains("verr_already_exists") // VirtualBox: the target VM folder already exists
        || l.contains("could not rename") // the import can't rename its folder to the taken name
        || l.contains("is locked") // Vagrant: "the machine is locked" (a dead process's lock)
        || l.contains("different provider") // "an active machine was found with a different provider"
        || l.contains("single provider at a time")
}

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
pub(super) fn has_machines(dir: &Path) -> bool {
    let Ok(machines) = std::fs::read_dir(dir.join(".vagrant").join("machines")) else { return false };
    machines.flatten().any(|m| std::fs::read_dir(m.path()).is_ok_and(|provs| provs.flatten().any(|p| p.path().join("id").is_file())))
}

/// Whether Vagrant recorded a machine of this folder under a provider other than `requested`.
fn recorded_on_other_provider(dir: &Path, requested: Provider) -> bool {
    let machines = dir.join(".vagrant").join("machines");
    let Ok(entries) = std::fs::read_dir(&machines) else { return false };
    let mut mismatch = false;
    for machine in entries.flatten() {
        if let Ok(provs) = std::fs::read_dir(machine.path()) {
            for prov in provs.flatten() {
                if prov.path().is_dir() && prov.file_name().to_string_lossy() != requested.id() {
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
fn folder_candidates(name: &str) -> [String; 2] {
    [name.to_string(), name.replace('\u{00b7}', "-")]
}

/// The VM names Isoloom wrote into the Vagrantfile, used to scope orphan cleanup strictly to
/// this lab's own VMs. Every provider block names the VM on a `v.name` / `v.guest_name` /
/// `v.vmx["displayName"]` line, as the quoted string after the `=` (for `displayName` the key
/// itself is quoted, so taking the value after `=` is what's correct, not the first quote).
pub(super) fn vm_names(dir: &Path) -> Vec<String> {
    vm_names_from(&std::fs::read_to_string(dir.join("Vagrantfile")).unwrap_or_default())
}

pub(super) fn vm_names_from(text: &str) -> Vec<String> {
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
fn parse_machine_folder(out: &str) -> Option<PathBuf> {
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
fn parse_vbox_vms(out: &str) -> Vec<(String, String)> {
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
fn remove_leftover_folders(base: &Path, wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
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
fn ours_in_prlctl<'a>(out: &'a str, wanted: &HashSet<String>) -> Vec<&'a str> {
    out.lines().map(str::trim).filter(|name| !name.is_empty() && wanted.contains(*name)).collect()
}

/// Removes VirtualBox VMs and leftover machine folders that belong to this lab (name matches one
/// of `wanted` EXACTLY) but that `vagrant destroy` can't reach: a VM a failed import half-created
/// and never registered with Vagrant, or an orphaned folder that makes the next import fail with
/// VERR_ALREADY_EXISTS. Exact-match only — a lossy match could delete an unrelated VM (e.g. a
/// user's own "lab-dc01" vs this lab's "lab · dc01"), which `unregistervm --delete` can't undo.
pub(super) async fn recover_virtualbox(wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
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
pub(super) async fn recover_parallels(wanted: &HashSet<String>, log: &mut impl FnMut(String)) {
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

// `vagrant status --machine-readable` lines: timestamp,target,type,data...
fn parse_status(out: &str) -> Vec<Machine> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ',');
            let (_ts, target, kind, data) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            (kind == "state" && !target.is_empty()).then(|| Machine {
                infra: target == CONTROLLER,
                name: target.to_string(),
                state: data.to_string(),
                image: String::new(),
                ip: String::new(),
                ports: Vec::new(),
                interfaces: Vec::new(),
                services: Vec::new(),
            })
        })
        .collect()
}

pub async fn status(dir: &Path, env: &[(String, String)]) -> Result<LabStatus> {
    let out = run_env_timed("vagrant", &["status", "--machine-readable"], Some(dir), env, STATUS_TIMEOUT).await?;
    Ok(status_from(&out, dir))
}

/// The status of the VMs in `dir` from `vagrant status --machine-readable` output.
fn status_from(out: &str, dir: &Path) -> LabStatus {
    let mut machines = parse_status(out);
    // The lab is up when its targets are; the controller is powered off once they are built.
    let targets = machines.iter().filter(|m| !m.infra);
    let running = targets.clone().count() > 0 && targets.clone().all(|m| m.state == "running");
    // The network diagram, as a Docker lab gets it: addresses are static (declared in the lab),
    // so it comes from the lab's spec (`.isoloom/vagrant` sits two levels under the lab).
    let networks = match dir.parent().and_then(Path::parent).and_then(|lab| super::lab::spec(lab).ok()) {
        Some(spec) => {
            let (mut ifaces, networks) = super::lab::topology(&spec);
            for m in &mut machines {
                if let Some(list) = ifaces.remove(&m.name) {
                    m.ip = list.first().map(|i| i.ip.clone()).unwrap_or_default();
                    m.interfaces = list;
                }
            }
            networks
        }
        None => Vec::new(),
    };
    // The hypervisor Vagrant runs them on (`provider-name` lines), e.g. "virtualbox".
    let provider = out.lines().find_map(|line| {
        let mut parts = line.splitn(4, ',');
        let (_ts, _target, kind, data) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        (kind == "provider-name" && !data.is_empty()).then(|| data.to_string())
    });
    LabStatus { running, parked: None, machines, networks, url: None, host: None, expires_at: None, place: None, provider, attacker: None }
}

/// Each machine's lab interfaces and the lab's network segments, read from the generated
/// Vagrantfile: a `config.vm.define "<machine>"` block, then its `private_network` lines with
/// `ip:`, `netmask:` and the VirtualBox internal network `isoloom-<lab>-<net>` (`<net>` being the
/// lab's own network name). The subnet is the address masked.
pub(super) fn vagrant_topology(text: &str, lab: &str) -> (std::collections::HashMap<String, Vec<super::model::Interface>>, Vec<super::model::Network>) {
    let prefix = format!("isoloom-{lab}-");
    let mut ifaces: std::collections::HashMap<String, Vec<super::model::Interface>> = std::collections::HashMap::new();
    let mut networks: Vec<super::model::Network> = Vec::new();
    let mut machine: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("config.vm.define ") {
            machine = quoted(rest);
            continue;
        }
        let Some(name) = machine.as_deref() else { continue };
        if !l.contains("private_network") {
            continue;
        }
        let (Some(ip), Some(intnet)) = (field(l, "ip:"), field(l, "virtualbox__intnet:")) else { continue };
        let net = intnet.strip_prefix(&prefix).unwrap_or(&intnet).to_string();
        let mask = field(l, "netmask:").unwrap_or_else(|| "255.255.255.0".into());
        if !networks.iter().any(|n| n.name == net) {
            // A VM's private network is a switch between the lab's machines; internet, when the
            // lab allows it, goes out through each VM's own NAT adapter, not through this segment.
            networks.push(super::model::Network { name: net.clone(), subnet: cidr(&ip, &mask).unwrap_or_default(), internal: false });
        }
        ifaces.entry(name.to_string()).or_default().push(super::model::Interface { network: net, ip });
    }
    (ifaces, networks)
}

/// The first double-quoted string in `s`.
fn quoted(s: &str) -> Option<String> {
    let open = s.find('"')?;
    let len = s[open + 1..].find('"')?;
    Some(s[open + 1..open + 1 + len].to_string())
}

/// The quoted value of `key: "..."` in a Vagrantfile line.
fn field(line: &str, key: &str) -> Option<String> {
    let at = line.find(key)?;
    quoted(&line[at + key.len()..])
}

/// `192.168.56.30` + `255.255.255.0` -> `192.168.56.0/24`.
fn cidr(ip: &str, mask: &str) -> Option<String> {
    let ip: std::net::Ipv4Addr = ip.parse().ok()?;
    let mask: std::net::Ipv4Addr = mask.parse().ok()?;
    let (i, m) = (u32::from(ip), u32::from(mask));
    Some(format!("{}/{}", std::net::Ipv4Addr::from(i & m), m.count_ones()))
}

#[cfg(test)]
mod tests {
    use super::{folder_candidates, is_stale_state_error, on_qemu, parse_status, vm_names_from};

    #[test]
    fn tells_a_lab_on_qemu_from_vagrant_s_machine_folders() {
        let dir = std::env::temp_dir().join(format!("cyberctf-qemu-{}", std::process::id()));
        let machines = dir.join(".vagrant").join("machines");
        std::fs::create_dir_all(machines.join("dc01").join("virtualbox")).unwrap();
        assert!(!on_qemu(&dir));
        std::fs::create_dir_all(machines.join("isoloom-controller").join("qemu")).unwrap();
        assert!(on_qemu(&dir), "the controller stays on as the attacker there");
        std::fs::remove_dir_all(&dir).ok();
        assert!(!on_qemu(&dir), "never started");
    }

    #[test]
    fn parses_machine_readable_status() {
        let out = "1700000000,pfsense-1,metadata,provider,virtualbox\n\
                   1700000000,pfsense-1,state,running\n\
                   1700000000,debian-1,state,poweroff\n\
                   1700000000,,ui,info,Current machine states:\n";
        let machines = parse_status(out);
        assert_eq!(machines.len(), 2);
        assert_eq!(machines[0].name, "pfsense-1");
        assert_eq!(machines[1].state, "poweroff");
    }

    #[test]
    fn the_controller_is_infra_and_does_not_decide_whether_the_lab_runs() {
        let out = "1,dc01,state,running\n1,ws01,state,running\n1,isoloom-controller,state,poweroff\n";
        let machines = super::parse_status(out);
        assert!(machines.iter().find(|m| m.name == "isoloom-controller").unwrap().infra);
        assert!(!machines.iter().find(|m| m.name == "dc01").unwrap().infra);
        // Every target is up while the (powered-off) controller is ignored.
        let targets = machines.iter().filter(|m| !m.infra);
        assert!(targets.clone().count() > 0 && targets.clone().all(|m| m.state == "running"));
    }

    #[test]
    fn ansible_verdict_keeps_the_fatal_line_without_vagrants_prefix() {
        // Regression: a MINILAB deploy failed with ws01 unreachable over WinRM, and the UI only got
        // Vagrant's "The SSH command responded with a non-zero exit status". The reason is this
        // line, which Vagrant prefixes with the machine that printed it.
        let line = "    isoloom-controller: fatal: [ws01]: UNREACHABLE! => {\"changed\": false, \"msg\": \"winrm connection error: Read timed out.\", \"unreachable\": true}";
        let v = super::ansible_verdict(line).expect("a fatal line");
        assert!(v.starts_with("fatal: [ws01]: UNREACHABLE!"), "{v}");
        assert!(!v.contains("isoloom-controller: "), "{v}");
        // Ordinary output is not a verdict; a huge line is cut to stay readable.
        assert_eq!(super::ansible_verdict("    dc01: ok: [dc01]"), None);
        assert_eq!(super::ansible_verdict("==> ws01: Running provisioner: ansible (shell)..."), None);
        let long = format!("fatal: [a]: FAILED! => {}", "x".repeat(400));
        assert_eq!(super::ansible_verdict(&long).unwrap().len(), 300);
    }

    #[test]
    fn vagrant_topology_reads_each_machines_lab_interface_and_the_subnet() {
        // The shape isoloom writes: a define block, then its private_network line.
        let vf = r#"
  config.vm.define "dc01" do |m|
    m.vm.network "private_network", ip: "192.168.56.30", netmask: "255.255.255.0", virtualbox__intnet: "isoloom-minilab-lab", libvirt__network_name: "isoloom-minilab-lab", libvirt__dhcp_enabled: false
  end
  config.vm.define "isoloom-controller" do |m|
    m.vm.network "private_network", ip: "192.168.56.253", netmask: "255.255.255.0", virtualbox__intnet: "isoloom-minilab-lab"
  end
"#;
        let (ifaces, nets) = super::vagrant_topology(vf, "minilab");
        assert_eq!(ifaces["dc01"].len(), 1);
        assert_eq!(ifaces["dc01"][0].network, "lab");
        assert_eq!(ifaces["dc01"][0].ip, "192.168.56.30");
        assert_eq!(ifaces["isoloom-controller"][0].ip, "192.168.56.253");
        // One segment, named by the lab's own network name (the isoloom-<lab>- prefix stripped),
        // with its subnet computed from the address and mask.
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].name, "lab");
        assert_eq!(nets[0].subnet, "192.168.56.0/24");
    }

    #[test]
    fn vagrant_topology_keeps_a_foreign_intnet_name_whole() {
        // An internal network not scoped to this lab is shown by its full name, never mangled.
        let vf = "config.vm.define \"a\" do |m|\n  m.vm.network \"private_network\", ip: \"10.0.0.5\", virtualbox__intnet: \"other-net\"\nend\n";
        let (ifaces, nets) = super::vagrant_topology(vf, "minilab");
        assert_eq!(ifaces["a"][0].network, "other-net");
        assert_eq!(nets[0].name, "other-net");
        assert_eq!(nets[0].subnet, "10.0.0.0/24"); // default mask when none is written
    }

    #[test]
    fn cidr_masks_the_address() {
        assert_eq!(super::cidr("192.168.56.30", "255.255.255.0").as_deref(), Some("192.168.56.0/24"));
        assert_eq!(super::cidr("10.20.0.31", "255.255.0.0").as_deref(), Some("10.20.0.0/16"));
        assert_eq!(super::cidr("bad", "255.255.255.0"), None);
    }

    #[test]
    fn vm_names_reads_the_value_for_every_provider_block() {
        // VirtualBox/Parallels/UTM use `v.name`, ESXi uses `v.guest_name`, VMware uses the quoted
        // key `v.vmx["displayName"]` — the parser must take the VALUE after `=`, not the key.
        let vf = r#"
  config.vm.define "dc01" do |m|
    m.vm.provider "virtualbox" do |v|
      v.name = "minilab · dc01"
    end
    m.vm.provider "vmware_desktop" do |v|
      v.vmx["displayName"] = "minilab · dc01"
    end
    m.vm.provider "vmware_esxi" do |v|
      v.guest_name = "minilab-dc01"
    end
  end
"#;
        let names = vm_names_from(vf);
        assert!(names.contains(&"minilab · dc01".to_string()), "got {names:?}");
        assert!(names.contains(&"minilab-dc01".to_string()), "got {names:?}");
        // The literal key must never be captured as a name.
        assert!(!names.iter().any(|n| n == "displayName"), "captured the key: {names:?}");
    }

    #[test]
    fn folder_candidates_cover_virtualbox_sanitization_exactly() {
        // VirtualBox turns the middot into a hyphen for the on-disk folder; both are exact.
        let c = folder_candidates("minilab · dc01");
        assert!(c.contains(&"minilab · dc01".to_string()));
        assert!(c.contains(&"minilab - dc01".to_string()));
        // It must NOT produce a collapsed form that could match an unrelated "minilab-dc01".
        assert!(!c.iter().any(|f| f == "minilab-dc01"));
    }

    #[test]
    fn stale_state_matches_only_real_startup_errors() {
        assert!(is_stale_state_error("VBoxManage: error: Details: code VERR_ALREADY_EXISTS"));
        assert!(is_stale_state_error("Could not rename the directory"));
        assert!(is_stale_state_error("the machine is locked! This means"));
        assert!(is_stale_state_error("An active machine was found with a different provider"));
        // Benign provisioner output must NOT trigger a destroy+retry of a healthy VM.
        assert!(!is_stale_state_error("group 'docker' already exists"));
        assert!(!is_stale_state_error("user 'vagrant' already exists, skipping"));
        assert!(!is_stale_state_error("network lab already registered"));
    }

    #[test]
    fn keeps_every_state_label_verbatim() {
        // Vagrant reports many lifecycle states; each is surfaced as-is for the UI.
        let out = "ts,a,state,saved\n\
                   ts,b,state,aborted\n\
                   ts,c,state,not_created\n\
                   ts,d,state,running\n";
        let m = parse_status(out);
        let states: Vec<(&str, &str)> = m.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
        assert_eq!(states, vec![("a", "saved"), ("b", "aborted"), ("c", "not_created"), ("d", "running")]);
    }

    #[test]
    fn running_requires_every_machine_up() {
        // The `status()` running rule: all machines must be "running".
        let up = parse_status("ts,a,state,running\nts,b,state,running\n");
        assert!(!up.is_empty() && up.iter().all(|m| m.state == "running"));
        let mixed = parse_status("ts,a,state,running\nts,b,state,poweroff\n");
        assert!(!mixed.iter().all(|m| m.state == "running"));
    }

    #[test]
    fn ignores_non_state_lines_and_empty_or_malformed_output() {
        // Only `state` rows with a non-empty target become machines.
        let out = "ts,,ui,info,Current machine states:\n\
                   ts,box,metadata,provider,virtualbox\n\
                   ts,,state,running\n\
                   short,line\n";
        assert!(parse_status(out).is_empty());
        assert!(parse_status("").is_empty());
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-vm-{tag}-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const CONTROLLED: &str = "Vagrant.configure(\"2\") do |config|\n  config.vm.define \"isoloom-controller\" do |m|\n  end\nend\n";

    #[test]
    fn a_controller_is_read_from_its_define_line() {
        let dir = temp_dir("ctl");
        assert!(!super::has_controller(&dir), "no Vagrantfile");
        std::fs::write(dir.join("Vagrantfile"), "config.vm.define \"dc01\" do |m|\nend\n").unwrap();
        assert!(!super::has_controller(&dir));
        std::fs::write(dir.join("Vagrantfile"), CONTROLLED).unwrap();
        assert!(super::has_controller(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn no_controller_or_one_on_qemu_is_never_powered_off() {
        let dir = temp_dir("park-ctl");
        let mut lines = Vec::new();
        super::park_controller(&dir, &[], &mut |l| lines.push(l)).await;
        // On QEMU the controller is the attacker: it stays on (no Vagrant run at all).
        std::fs::write(dir.join("Vagrantfile"), CONTROLLED).unwrap();
        std::fs::create_dir_all(dir.join(".vagrant").join("machines").join(super::CONTROLLER).join("qemu")).unwrap();
        super::park_controller(&dir, &[], &mut |l| lines.push(l)).await;
        assert!(lines.is_empty(), "{lines:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn discarding_saved_state_needs_named_vms() {
        // No Vagrantfile, so no VM names: nothing is asked of VirtualBox.
        let dir = temp_dir("discard");
        let mut lines = Vec::new();
        super::discard_aborted_saved(&dir, &mut |l| lines.push(l)).await;
        assert!(lines.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failure_leads_with_ansibles_verdict() {
        use crate::error::Error;
        let failed = || Err(Error::CommandFailed { command: "vagrant up".into(), stderr: "non-zero exit status".into() });
        match super::lead_with_verdict(failed(), Some("fatal: [ws01]: UNREACHABLE!".into())) {
            Err(Error::CommandFailed { command, stderr }) => {
                assert_eq!(command, "vagrant up");
                assert_eq!(stderr, "fatal: [ws01]: UNREACHABLE!\nnon-zero exit status");
            }
            other => panic!("{other:?}"),
        }
        // No verdict: the error as it was; success stays success; other errors are untouched.
        match super::lead_with_verdict(failed(), None) {
            Err(Error::CommandFailed { stderr, .. }) => assert_eq!(stderr, "non-zero exit status"),
            other => panic!("{other:?}"),
        }
        assert!(super::lead_with_verdict(Ok(()), Some("fatal: [x]".into())).is_ok());
        assert!(matches!(super::lead_with_verdict(Err(Error::Invalid("x".into())), Some("v".into())), Err(Error::Invalid(m)) if m == "x"));
    }

    #[test]
    fn parking_suspends_or_halts_and_resuming_keeps_the_provider() {
        use super::super::model::Park;
        assert_eq!(super::park_verb(Park::Pause), "suspend");
        assert_eq!(super::park_verb(Park::Shutdown), "halt");
        assert_eq!(super::resume_args(None), ["up", "--no-provision"]);
        assert_eq!(super::resume_args(Some("virtualbox")), ["up", "--no-provision", "--provider", "virtualbox"]);
    }

    #[test]
    fn ansible_verdict_cuts_long_non_ascii_lines_on_a_char_boundary() {
        let long = format!("fatal: [a]: FAILED! => {}", "é".repeat(300));
        let v = super::ansible_verdict(&long).unwrap();
        assert!(v.ends_with("..."));
        assert!(v.len() <= 300);
        // A machine prefix with a space is not Vagrant's, so not a verdict.
        assert_eq!(super::ansible_verdict("two words: fatal: [a]: FAILED!"), None);
        assert_eq!(super::ansible_verdict("fatal: [a]: FAILED!").as_deref(), Some("fatal: [a]: FAILED!"));
    }

    #[test]
    fn another_recorded_provider_is_a_mismatch() {
        use super::super::providers::Provider;
        let dir = temp_dir("reconcile");
        assert!(!super::recorded_on_other_provider(&dir, Provider::Virtualbox), "never started");
        let machines = dir.join(".vagrant").join("machines");
        std::fs::create_dir_all(machines.join("dc01").join("virtualbox")).unwrap();
        // A stray file is not a provider folder.
        std::fs::write(machines.join("dc01").join("index_uuid"), "x").unwrap();
        assert!(!super::recorded_on_other_provider(&dir, Provider::Virtualbox));
        assert!(super::recorded_on_other_provider(&dir, Provider::VmwareEsxi));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn reconciling_on_the_same_provider_keeps_the_state() {
        use super::super::providers::Provider;
        let dir = temp_dir("reconcile-same");
        let mut lines = Vec::new();
        super::reconcile_provider(&dir, Provider::Virtualbox, &[], &mut |l| lines.push(l)).await;
        std::fs::create_dir_all(dir.join(".vagrant").join("machines").join("dc01").join("virtualbox")).unwrap();
        super::reconcile_provider(&dir, Provider::Virtualbox, &[], &mut |l| lines.push(l)).await;
        assert!(lines.is_empty());
        assert!(dir.join(".vagrant").is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_virtualboxs_machine_folder() {
        let out = "API version: 7_0\nDefault machine folder:          /Users/a/VirtualBox VMs\nVRDE auth library: x\n";
        assert_eq!(super::parse_machine_folder(out), Some(std::path::PathBuf::from("/Users/a/VirtualBox VMs")));
        assert_eq!(super::parse_machine_folder("Default machine folder:   \n"), None);
        assert_eq!(super::parse_machine_folder(""), None);
    }

    #[test]
    fn lists_virtualbox_vms_by_name_and_uuid() {
        let out = "\"minilab · dc01\" {1111-aaaa}\n\"other\" {2222-bbbb}\ngarbage\n} backwards {\n";
        assert_eq!(super::parse_vbox_vms(out), vec![("minilab · dc01".to_string(), "1111-aaaa".to_string()), ("other".to_string(), "2222-bbbb".to_string())]);
    }

    #[test]
    fn only_our_parallels_vms_are_picked() {
        let wanted: std::collections::HashSet<String> = ["minilab · dc01".to_string()].into();
        let out = "minilab · dc01\n  \nminilab-dc01\nsomeone else\n";
        assert_eq!(super::ours_in_prlctl(out, &wanted), ["minilab · dc01"]);
    }

    #[test]
    fn leftover_folders_go_by_their_exact_sanitized_names() {
        let base = temp_dir("vbox-folders");
        for f in ["minilab - dc01", "minilab-dc01", "unrelated"] {
            std::fs::create_dir_all(base.join(f)).unwrap();
        }
        std::fs::write(base.join("minilab · dc01"), "a file, not a folder").unwrap();
        let wanted: std::collections::HashSet<String> = ["minilab · dc01".to_string()].into();
        let mut lines = Vec::new();
        super::remove_leftover_folders(&base, &wanted, &mut |l| lines.push(l));
        assert!(!base.join("minilab - dc01").exists());
        assert!(base.join("minilab-dc01").is_dir(), "a lossy match is never deleted");
        assert!(base.join("unrelated").is_dir());
        assert!(base.join("minilab · dc01").is_file());
        assert_eq!(lines, ["Removing a leftover VM folder: minilab - dc01"]);
        // A missing base folder is nothing to do.
        super::remove_leftover_folders(&base.join("nope"), &wanted, &mut |l| lines.push(l));
        assert_eq!(lines.len(), 1);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn status_without_a_lab_spec_has_machines_and_the_provider() {
        let dir = temp_dir("status");
        let out = "1,dc01,provider-name,virtualbox\n1,dc01,state,running\n1,isoloom-controller,state,poweroff\n";
        let s = super::status_from(out, &dir);
        assert!(s.running, "the targets are up, the controller doesn't count");
        assert_eq!(s.provider.as_deref(), Some("virtualbox"));
        assert_eq!(s.machines.len(), 2);
        assert!(s.networks.is_empty());
        // Only the controller: nothing of the lab is running.
        let s = super::status_from("1,isoloom-controller,state,running\n", &dir);
        assert!(!s.running);
        assert_eq!(s.provider, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn status_reads_the_network_diagram_from_the_labs_spec() {
        let lab = temp_dir("status-lab");
        std::fs::write(
            lab.join("isoloom.yml"),
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    docker: { image: nginx:1.27 }\n",
        )
        .unwrap();
        let vagrant = lab.join(".isoloom").join("vagrant");
        std::fs::create_dir_all(&vagrant).unwrap();
        let s = super::status_from("1,web,state,running\n1,other,state,running\n", &vagrant);
        assert!(s.running);
        let web = s.machines.iter().find(|m| m.name == "web").unwrap();
        assert_eq!(web.ip, "10.30.0.10");
        assert_eq!(web.interfaces.len(), 1);
        assert_eq!(s.machines.iter().find(|m| m.name == "other").unwrap().ip, "");
        assert_eq!(s.networks.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["lab"]);
        std::fs::remove_dir_all(lab).unwrap();
    }

    #[test]
    fn vm_names_come_from_the_vagrantfile_on_disk() {
        let dir = temp_dir("names");
        assert!(super::vm_names(&dir).is_empty());
        std::fs::write(dir.join("Vagrantfile"), "v.name = \"b\"\nv.name = \"a\"\nv.name = \"a\"\nv.name = \"\"\nv.name\n").unwrap();
        assert_eq!(super::vm_names(&dir), ["a", "b"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quoted_and_field_read_vagrantfile_values() {
        assert_eq!(super::quoted("x \"a\" \"b\"").as_deref(), Some("a"));
        assert_eq!(super::quoted("no quotes"), None);
        assert_eq!(super::quoted("\"open"), None);
        assert_eq!(super::field("ip: \"10.0.0.1\", netmask: \"255.0.0.0\"", "netmask:").as_deref(), Some("255.0.0.0"));
        assert_eq!(super::field("ip: \"10.0.0.1\"", "netmask:"), None);
    }

    #[test]
    fn topology_skips_lines_outside_a_machine_or_without_an_address() {
        let vf = "m.vm.network \"private_network\", ip: \"10.0.0.9\", virtualbox__intnet: \"isoloom-l-lab\"\n\
                  config.vm.define \"a\" do |m|\n\
                  m.vm.network \"forwarded_port\", guest: 80\n\
                  m.vm.network \"private_network\", ip: \"10.0.0.5\"\n\
                  m.vm.network \"private_network\", ip: \"10.0.0.5\", netmask: \"bad\", virtualbox__intnet: \"isoloom-l-lab\"\n";
        let (ifaces, nets) = super::vagrant_topology(vf, "l");
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces["a"].len(), 1);
        assert_eq!(nets[0].subnet, "", "an unreadable mask leaves the subnet blank");
    }
}

#[cfg(test)]
mod proptests;
