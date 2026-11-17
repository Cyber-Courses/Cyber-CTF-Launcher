//! The VM self-test: a real throwaway VM on a local hypervisor.

mod leftover;
mod networks;

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use super::Reporter;
use super::downloads::ensure_box;
use crate::error::{Error, Result};
use crate::platform::exec::{Event, Step, Vagrant, run, within};
use crate::platform::host::Arch;
use crate::platform::tool_output::ping::Ping;
use crate::runtime::providers::{self, Provider};
use crate::runtime::selftest_report::StepState;

/// The test VM's address on its private lab network (VirtualBox's default host-only range):
/// the first of these that nothing answers on yet. One that answers already (another VM on it)
/// would make the ping check pass whatever the test VM does.
const VM_IPS: [&str; 4] = ["192.168.56.250", "192.168.56.249", "192.168.56.248", "192.168.56.247"];
/// Booting (the box imported, the VM up, the command inside) and deleting the test VM: past
/// these the hypervisor or Vagrant is stuck, and the test says so instead of hanging.
const BOOT_LIMIT: Duration = Duration::from_secs(1200);
const DESTROY_LIMIT: Duration = Duration::from_secs(300);
/// How the test VM shows up in the hypervisor's own app while it runs.
const VM_NAME: &str = "CyberCTF test VM";
/// Pings (1 s apart) for the VM to answer on the lab network.
const PING_TRIES: u32 = 5;

/// The local provider the VM test uses ([`providers::pick`] of the hypervisors ready here, Vagrant
/// included): the preferred one (Settings) when it's ready, else the first ready one.
pub(super) async fn test_provider(preferred: Option<Provider>) -> Option<Provider> {
    providers::pick(preferred, &providers::ready_here().await)
}

/// Why no local hypervisor can run the test VM, of `statuses` ([`providers::detect`]'s): what keeps
/// the preferred one (else the first one installed) from running VMs, in the words the machine
/// check uses (`/dev/kvm` this account can't open, a group that applies from the next login…).
fn none_ready(preferred: Option<Provider>, statuses: &[providers::ProviderStatus]) -> String {
    let installed = |s: &&providers::ProviderStatus| !s.remote && s.hypervisor == Some(true);
    let blocked = statuses
        .iter()
        .filter(installed)
        .find(|s| preferred.is_some_and(|p| s.provider == p))
        .or_else(|| statuses.iter().find(installed))
        .and_then(|s| s.reason.as_ref().map(|why| (Provider::from(s.provider), why)));
    match blocked {
        Some((p, why)) => format!("{} can't run VMs here: {}", p.label(), providers::said(why)),
        None => "no local hypervisor with its Vagrant plugin is installed".to_owned(),
    }
}

/// A read-only command that only succeeds when the hypervisor's service is up and usable
/// by this user (not just installed). UTM has none: it is probed as a lab's start does
/// ([`check_utm`](crate::runtime::launch::check_utm)), which opens it hidden when it isn't
/// running, where `utmctl list` would open it with its window in front.
fn responds(p: Provider) -> Option<(&'static str, &'static [&'static str])> {
    match p {
        Provider::Virtualbox => Some(("VBoxManage", &["list", "hostinfo"])),
        Provider::VmwareDesktop => Some(("vmrun", &["list"])),
        Provider::Parallels => Some(("prlctl", &["list", "--all"])),
        Provider::Libvirt => Some(("virsh", &["-c", "qemu:///system", "list", "--all"])),
        _ => None,
    }
}

/// The local hypervisors the VM self-test can use: every one of [`Provider::ALL`] but the servers.
pub(super) fn test_providers() -> impl Iterator<Item = Provider> {
    Provider::ALL.into_iter().filter(|p| !p.is_remote())
}

/// Small public boxes that exist for this provider and CPU, smallest first.
pub(crate) fn candidate_boxes(p: Provider, arch: Arch) -> &'static [&'static str] {
    match (p, arch) {
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, Arch::X86_64) => &["generic/alpine319", "bento/debian-12"],
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, Arch::Aarch64) => &["bento/debian-12"],
        (Provider::Utm, _) => &["utm/bookworm"],
        _ => &["generic/alpine319"],
    }
}

/// The Vagrant box provider a box is added and listed under for `p`. QEMU is the exception: the
/// vagrant-qemu plugin boots a libvirt-format box's qcow2 directly, so its box is stored as
/// `libvirt`, never `qemu`. `generic/alpine319`'s own "qemu" box is a libvirt image mislabeled
/// (`box add --provider qemu` fetches it and then rejects it), while its `libvirt` box bakes in
/// the vagrant user and key, so the test VM gets SSH without cloud-init (which vagrant-qemu does
/// not seed). This mirrors the Isoloom engine, which runs its QEMU labs from libvirt boxes too.
pub(crate) fn box_provider(p: Provider) -> &'static str {
    match p {
        Provider::Qemu => "libvirt",
        _ => p.id(),
    }
}

/// Providers whose Vagrant plugin can put the VM on a static private network.
fn has_private_network(p: Provider) -> bool {
    matches!(p, Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels | Provider::Libvirt)
}

/// Picks a candidate box already downloaded for this provider, else the first candidate.
/// Returns (box, already downloaded).
pub(super) async fn pick_box(p: Provider, arch: Arch) -> (&'static str, bool) {
    let candidates = candidate_boxes(p, arch);
    let list = run("vagrant", &["box", "list"], None).await.unwrap_or_default();
    match candidates.iter().find(|c| box_listed(&list, c, box_provider(p), arch)) {
        Some(c) => (c, true),
        None => (candidates[0], false),
    }
}

/// Whether `vagrant box list` has `name` for `provider` and this CPU (a box with no
/// architecture in the listing counts for any).
fn box_listed(list: &str, name: &str, provider: &str, arch: Arch) -> bool {
    let arch = arch.docker_name();
    crate::machine::storage::owned::listed_boxes(list).iter().any(|b| {
        let other_cpu = matches!(b.arch.as_deref(), Some(a @ ("amd64" | "arm64")) if a != arch);
        b.owned.name == name && b.owned.provider == provider && !other_cpu
    })
}

/// The test VM boots as a lab's VMs do: on QEMU with the hypervisor's acceleration (vagrant-qemu's
/// default, `accel=kvm` on Linux, as every lab's Vagrantfile asks), never software emulation,
/// which booted without `/dev/kvm` and passed where no lab could start.
/// `ip`: the VM's address on a private network, when it gets one. `dir`: the test's folder, which
/// a VirtualBox VM's description names, so a VM an earlier test left is known as the test's
/// ([`leftover`]).
fn vagrantfile(p: Provider, bx: &str, ip: Option<&str>, dir: &Path) -> String {
    let mut v = format!(
        "Vagrant.configure(\"2\") do |config|\n  config.vm.box = \"{bx}\"\n  config.vm.hostname = \"cyberctf-selftest\"\n  config.vm.boot_timeout = 600\n  config.vm.synced_folder \".\", \"/vagrant\", disabled: true\n"
    );
    if let Some(ip) = ip.filter(|_| has_private_network(p)) {
        let _ = writeln!(v, "  config.vm.network \"private_network\", ip: \"{ip}\"");
    }
    // With several virtual switches and none named, Vagrant would stop to ask which one.
    if p == Provider::Hyperv {
        v.push_str(&crate::platform::exec::hyperv_switch_line("  ", "config"));
    }
    // A readable name in the hypervisor's VM list instead of Vagrant's `<dir>_default_<timestamp>`.
    let settings = match p {
        Provider::Virtualbox => format!(
            "    h.name = \"{VM_NAME}\"\n    h.memory = 512\n    h.customize [\"modifyvm\", :id, \"--description\", {}]\n",
            ruby_quote(&leftover::vbox_description(dir))
        ),
        Provider::Parallels => format!("    h.name = \"{VM_NAME}\"\n    h.memory = 512\n"),
        Provider::VmwareDesktop => format!("    h.vmx[\"displayName\"] = \"{VM_NAME}\"\n    h.memory = 512\n"),
        Provider::Libvirt => "    h.default_prefix = \"cyberctf-\"\n    h.memory = 512\n".to_string(),
        Provider::Hyperv => format!("    h.vmname = \"{VM_NAME}\"\n"),
        Provider::Utm => format!("    h.name = \"{VM_NAME}\"\n"),
        // vagrant-qemu's default is 4G, much more than a test VM needs.
        Provider::Qemu => "    h.memory = \"1G\"\n".to_string(),
        _ => String::new(),
    };
    if !settings.is_empty() {
        let _ = writeln!(v, "  config.vm.provider \"{}\" do |h|\n{settings}  end", p.id());
    }
    v.push_str("end\n");
    v
}

/// `e`, the test VM's failed start, as the system's refusal of the hypervisor's app when it is one
/// (UTM's start answered "Operation not permitted"): coded, with the way out, as a lab's start
/// says it; else as it was.
fn refused(provider: Provider, e: Error) -> Error {
    use crate::platform::tool_output::apple_events::refused_by_the_system;
    match &e {
        Error::CommandFailed { stderr, .. } if provider == Provider::Utm && refused_by_the_system(stderr) => {
            Error::coded(crate::error::Code::HypervisorNotPermitted { hypervisor: provider.label().to_owned() })
        }
        _ => e,
    }
}

/// `s` as a Ruby single-quoted string (a Windows path's backslashes kept as they are).
fn ruby_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// One ping with a 3 s wait, in this OS's `ping` flags.
fn ping_args(ip: &str) -> [&str; 5] {
    if cfg!(windows) {
        ["-n", "1", "-w", "3000", ip]
    } else if cfg!(target_os = "macos") {
        ["-c", "1", "-t", "3", ip]
    } else {
        ["-c", "1", "-W", "3", ip]
    }
}

/// Whether `ip` answers one ping: a reply in its output, not the exit code alone, which Windows'
/// `ping` gives as 0 for a router's "Destination host unreachable" ([`Ping`]).
async fn answers_ping(ip: &str) -> bool {
    run("ping", &ping_args(ip), None).await.is_ok_and(|out| Ping::of(&out) == Ping::Reply)
}

async fn destroy(dir: &Path) -> Result<String> {
    within("vagrant destroy -f", DESTROY_LIMIT, Vagrant::of(dir, &[]).run(&["destroy", "-f"])).await
}

/// The first test address nothing answers on yet (see [`VM_IPS`]).
async fn free_ip() -> Option<&'static str> {
    for ip in VM_IPS {
        if !answers_ping(ip).await {
            return Some(ip);
        }
    }
    None
}

/// What the command inside the test VM prints its answer after, so it is told from anything
/// else in the output (stderr shares it: warnings, "Connection to … closed.").
const UNAME_TAG: &str = "cyberctf-uname:";

/// The command run inside the test VM.
fn uname_command() -> String {
    format!("echo {UNAME_TAG}$(uname -srm)")
}

/// What `uname -srm` printed, out of the command's output.
fn uname_line(out: &str) -> String {
    out.lines().find_map(|l| l.trim().strip_prefix(UNAME_TAG)).unwrap_or("").trim().to_string()
}

pub(super) async fn run_test(dir: &Path, preferred: Option<Provider>, r: &Reporter) -> Result<()> {
    clear_earlier_test(dir, r).await?;
    // The host networks there before the VM boots, once its hypervisor is known: the ones the
    // test makes go after it (#353).
    let before = std::sync::OnceLock::new();
    let res = steps(dir, preferred, r, &before).await;
    // Destroy the VM whatever happened; the box stays cached for the next test.
    if dir.join("Vagrantfile").exists() {
        let down = destroy(dir).await;
        let deleted = down.is_ok();
        r.cleanup("Delete the test VM", &res, down, Some("test box kept for next time".into()));
        if let (true, Some((provider, before))) = (deleted, before.get()) {
            remove_networks(*provider, before, r).await;
        }
    }
    res
}

/// Removes the host networks the test made (see `networks`), said as a step when it removed one
/// or couldn't; the test's result stands either way.
async fn remove_networks(provider: Provider, before: &networks::Networks, r: &Reporter) {
    const STEP: (&str, &str) = ("networks", "Remove the networks the test made");
    match networks::remove_made_since(provider, before).await {
        Ok(removed) if removed.is_empty() => {}
        Ok(removed) => r.send(STEP.0, STEP.1, StepState::Ok, Some(removed.join(", "))),
        Err(e) => r.send(STEP.0, STEP.1, StepState::Fail, Some(e)),
    }
}

/// What a test interrupted last time (the window closed, the app quit or was uninstalled) may
/// have left: its VM, destroyed through Vagrant when Vagrant still knows it, else found by its
/// name and removed when it is surely this test's ([`leftover`]); then Vagrant's state of it.
/// Reported only when something was left.
async fn clear_earlier_test(dir: &Path, r: &Reporter) -> Result<()> {
    const STEP: (&str, &str) = ("leftover", "Remove the VM an earlier test left");
    let state = dir.join(".vagrant");
    if state.exists()
        && let Err(e) = destroy(dir).await
    {
        log::warn!("destroying the VM an earlier self-test left: {e}");
    }
    match leftover::clear(dir).await {
        Ok(removed) if removed.is_empty() => {}
        Ok(removed) => r.send(STEP.0, STEP.1, StepState::Ok, Some(format!("removed from {}", removed.join(", ")))),
        Err(e) => {
            r.send(STEP.0, STEP.1, StepState::Fail, Some(e.to_string()));
            return Err(e);
        }
    }
    if let Err(e) = crate::platform::fs::remove_dir_if_present(&state) {
        log::warn!("removing {}: {e}", state.display());
    }
    Ok(())
}

async fn steps(dir: &Path, preferred: Option<Provider>, r: &Reporter, before: &std::sync::OnceLock<(Provider, networks::Networks)>) -> Result<()> {
    let provider = host_provider(preferred, r).await?;
    let _ = before.set((provider, networks::Networks::here(provider).await));
    let arch = Arch::HERE;
    let (bx, _) = pick_box(provider, arch).await;
    r.step("box", "Get a small test VM image", async {
        let had = ensure_box(dir, provider, bx, arch, |l| r.progress("box", "Get a small test VM image", &l)).await?;
        Ok(((), Some(if had { format!("{bx} (ready)") } else { bx.to_string() })))
    })
    .await?;

    // An address for the ping check that nothing answers on before the VM exists.
    let ip = if has_private_network(provider) { free_ip().await } else { None };
    boot_and_exec(dir, provider, bx, ip, r).await?;
    network(provider, ip, r).await
}

/// Vagrant and a hypervisor that answers: the provider the test runs on.
async fn host_provider(preferred: Option<Provider>, r: &Reporter) -> Result<Provider> {
    r.step("vagrant", "Vagrant is installed", async {
        let v = run("vagrant", &["--version"], None).await?;
        Ok(((), Some(v.trim().to_string())))
    })
    .await?;

    let provider = r
        .step("provider", "A hypervisor is ready", async {
            let Some(p) = test_provider(preferred).await else {
                return Err(Error::Invalid(none_ready(preferred, &providers::detect(true).await)));
            };
            Ok((p, Some(p.id().to_string())))
        })
        .await?;

    r.step("hypervisor", "Hypervisor responds", async {
        match responds(provider) {
            None if provider == Provider::Utm => {
                crate::runtime::launch::check_utm(&mut |_| {}).await?;
                Ok(((), None))
            }
            Some((program, args)) => {
                run(program, args, None).await?;
                Ok(((), None))
            }
            None => Ok(((), Some("nothing to probe for this provider".into()))),
        }
    })
    .await?;

    Ok(provider)
}

/// Boots the test VM and runs a command inside. Both run as one batch: on Hyper-V (not
/// elevated) that is one administrator prompt for both.
async fn boot_and_exec(dir: &Path, provider: Provider, bx: &str, ip: Option<&'static str>, r: &Reporter) -> Result<()> {
    let mut inside = None;
    r.step("boot", "Boot the test VM", async {
        std::fs::write(dir.join("Vagrantfile"), vagrantfile(provider, bx, ip, dir))?;
        let steps = [Step::new(&["up", "--provider", provider.id()]), Step::new(&["ssh", "-c", &uname_command()])];
        let vagrant = Vagrant::new(dir, &[], provider == Provider::Hyperv);
        let batch = vagrant.batch(&steps, |e| {
            if let Event::Line(0, l) = e {
                r.progress("boot", "Boot the test VM", &l);
            }
        });
        let mut outcomes = within("vagrant up / vagrant ssh", BOOT_LIMIT, batch).await?;
        inside = outcomes.pop().flatten();
        outcomes.pop().flatten().unwrap_or_else(|| Err(Error::Invalid("the test VM was not started".into()))).map_err(|e| refused(provider, e))?;
        Ok(((), Some(format!("{bx} on {}", provider.id()))))
    })
    .await?;

    r.step("exec", "Run a command inside the VM", async {
        let out = inside.take().unwrap_or_else(|| Err(Error::Invalid("the command did not run".into())))?;
        Ok(((), Some(uname_line(&out))))
    })
    .await?;

    Ok(())
}

/// The VM answers ping on its private lab network (skipped without one, or a free address).
async fn network(provider: Provider, ip: Option<&'static str>, r: &Reporter) -> Result<()> {
    match ip {
        Some(ip) => {
            r.step("network", "VM reachable on a lab network", async {
                let mut last = String::new();
                for _ in 0..PING_TRIES {
                    match run("ping", &ping_args(ip), None).await {
                        Ok(out) if Ping::of(&out) == Ping::Reply => return Ok(((), Some(format!("this machine → {ip}")))),
                        Ok(out) => out.lines().map(str::trim).filter(|l| !l.is_empty()).nth(1).unwrap_or("no reply").clone_into(&mut last),
                        Err(e) => last = e.to_string(),
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(Error::Invalid(format!("{ip} did not answer ping: {last}")))
            })
            .await?;
        }
        None if has_private_network(provider) => {
            r.send("network", "VM reachable on a lab network", StepState::Skip, Some(format!("every test address ({}…) is taken on this machine", VM_IPS[0])));
        }
        None => r.send("network", "VM reachable on a lab network", StepState::Skip, Some(format!("{} has no private networks", provider.id()))),
    }

    Ok(())
}

#[cfg(test)]
mod tests;
