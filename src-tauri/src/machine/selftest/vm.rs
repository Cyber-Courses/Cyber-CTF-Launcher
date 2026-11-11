//! The VM self-test: a real throwaway VM on a local hypervisor.

mod leftover;

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use super::downloads::ensure_box;
use super::{Reporter, arm};
use crate::error::{Error, Result};
use crate::platform::exec::{Event, Step, Vagrant, run, within};
use crate::runtime::providers::{self, Provider};

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

/// A read-only command that only succeeds when the hypervisor's service is up and usable
/// by this user (not just installed).
fn responds(p: Provider) -> Option<(&'static str, &'static [&'static str])> {
    match p {
        Provider::Virtualbox => Some(("VBoxManage", &["list", "hostinfo"])),
        Provider::VmwareDesktop => Some(("vmrun", &["list"])),
        Provider::Parallels => Some(("prlctl", &["list", "--all"])),
        Provider::Libvirt => Some(("virsh", &["-c", "qemu:///system", "list", "--all"])),
        Provider::Utm => Some(("/Applications/UTM.app/Contents/MacOS/utmctl", &["list"])),
        _ => None,
    }
}

/// The local hypervisors the VM self-test can use: every one of [`Provider::ALL`] but the servers.
pub(super) fn test_providers() -> impl Iterator<Item = Provider> {
    Provider::ALL.into_iter().filter(|p| !p.is_remote())
}

/// Small public boxes that exist for this provider and CPU, smallest first.
pub(crate) fn candidate_boxes(p: Provider, arm: bool) -> &'static [&'static str] {
    match (p, arm) {
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, false) => &["generic/alpine319", "bento/debian-12"],
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, true) => &["bento/debian-12"],
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
pub(super) async fn pick_box(p: Provider, arm: bool) -> (&'static str, bool) {
    let candidates = candidate_boxes(p, arm);
    let list = run("vagrant", &["box", "list"], None).await.unwrap_or_default();
    match candidates.iter().find(|c| box_listed(&list, c, box_provider(p), arm)) {
        Some(c) => (c, true),
        None => (candidates[0], false),
    }
}

/// Whether `vagrant box list` has `name` for `provider` and this CPU (a box with no
/// architecture in the listing counts for any).
fn box_listed(list: &str, name: &str, provider: &str, arm: bool) -> bool {
    let arch = if arm { "arm64" } else { "amd64" };
    crate::machine::storage::owned::listed_boxes(list).iter().any(|b| {
        let other_cpu = matches!(b.arch.as_deref(), Some(a @ ("amd64" | "arm64")) if a != arch);
        b.owned.name == name && b.owned.provider == provider && !other_cpu
    })
}

/// `no_kvm`: a Linux host without a usable /dev/kvm. vagrant-qemu asks for `accel=kvm` with
/// `cpu=host` there, which QEMU refuses; the test VM falls back to software emulation (slow,
/// but it boots and the check says something true).
/// `ip`: the VM's address on a private network, when it gets one. `dir`: the test's folder, which
/// a VirtualBox VM's description names, so a VM an earlier test left is known as the test's
/// ([`leftover`]).
fn vagrantfile(p: Provider, bx: &str, no_kvm: bool, ip: Option<&str>, dir: &Path) -> String {
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
        Provider::Qemu if no_kvm => "    h.memory = \"1G\"\n    h.machine = \"q35,accel=tcg\"\n    h.cpu = \"max\"\n".to_string(),
        Provider::Qemu => "    h.memory = \"1G\"\n".to_string(),
        _ => String::new(),
    };
    if !settings.is_empty() {
        let _ = writeln!(v, "  config.vm.provider \"{}\" do |h|\n{settings}  end", p.id());
    }
    v.push_str("end\n");
    v
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

async fn destroy(dir: &Path) -> Result<String> {
    within("vagrant destroy -f", DESTROY_LIMIT, Vagrant::of(dir, &[]).run(&["destroy", "-f"])).await
}

/// The first test address nothing answers on yet (see [`VM_IPS`]).
async fn free_ip() -> Option<&'static str> {
    for ip in VM_IPS {
        if run("ping", &ping_args(ip), None).await.is_err() {
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
    let res = steps(dir, preferred, r).await;
    // Destroy the VM whatever happened; the box stays cached for the next test.
    if dir.join("Vagrantfile").exists() {
        let down = destroy(dir).await;
        r.cleanup("Delete the test VM", &res, down, Some("test box kept for next time".into()));
    }
    res
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
        Ok(removed) => r.send(STEP.0, STEP.1, "ok", Some(format!("removed from {}", removed.join(", ")))),
        Err(e) => {
            r.send(STEP.0, STEP.1, "fail", Some(e.to_string()));
            return Err(e);
        }
    }
    if let Err(e) = crate::platform::fs::remove_dir_if_present(&state) {
        log::warn!("removing {}: {e}", state.display());
    }
    Ok(())
}

async fn steps(dir: &Path, preferred: Option<Provider>, r: &Reporter) -> Result<()> {
    let provider = host_provider(preferred, r).await?;
    let arm = arm();
    let (bx, _) = pick_box(provider, arm).await;
    r.step("box", "Get a small test VM image", async {
        let had = ensure_box(dir, provider, bx, arm, |l| r.progress("box", "Get a small test VM image", &l)).await?;
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
            let p = test_provider(preferred).await.ok_or_else(|| Error::Invalid("no local hypervisor with its Vagrant plugin is installed".into()))?;
            Ok((p, Some(p.id().to_string())))
        })
        .await?;

    r.step("hypervisor", "Hypervisor responds", async {
        match responds(provider) {
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
        let no_kvm = cfg!(target_os = "linux") && providers::kvm_problem().is_some();
        std::fs::write(dir.join("Vagrantfile"), vagrantfile(provider, bx, no_kvm, ip, dir))?;
        let steps = [Step::new(&["up", "--provider", provider.id()]), Step::new(&["ssh", "-c", &uname_command()])];
        let vagrant = Vagrant::new(dir, &[], provider == Provider::Hyperv);
        let batch = vagrant.batch(&steps, |e| {
            if let Event::Line(0, l) = e {
                r.progress("boot", "Boot the test VM", &l);
            }
        });
        let mut outcomes = within("vagrant up / vagrant ssh", BOOT_LIMIT, batch).await?;
        inside = outcomes.pop().flatten();
        outcomes.pop().flatten().unwrap_or_else(|| Err(Error::Invalid("the test VM was not started".into())))?;
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
                        Ok(_) => return Ok(((), Some(format!("this machine → {ip}")))),
                        Err(e) => last = e.to_string(),
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(Error::Invalid(format!("{ip} did not answer ping: {last}")))
            })
            .await?;
        }
        None if has_private_network(provider) => {
            r.send("network", "VM reachable on a lab network", "skip", Some(format!("every test address ({}…) is taken on this machine", VM_IPS[0])));
        }
        None => r.send("network", "VM reachable on a lab network", "skip", Some(format!("{} has no private networks", provider.id()))),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_vm_test_picks_no_hypervisor_without_vagrant() {
        // Regression: the test's own copy of "ready" never asked whether Vagrant was there, and
        // picked a hypervisor whose every Vagrant call would then fail.
        use crate::platform::exec::fake::{Tools, ok};
        let tools = Tools::new(|program, _| if program == "vagrant" { None } else { ok("") });
        assert_eq!(test_provider(Some(Provider::Virtualbox)).await, None);
        assert!(tools.called("vagrant --version"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn the_test_vm_takes_an_address_nothing_answers_on_yet() {
        // Regression: the test VM always took .250, so another VM there made the ping pass.
        use crate::platform::exec::fake::{Tools, fail, ok};
        let _tools = Tools::new(|program, args| match (program, args.last().copied()) {
            ("ping", Some("192.168.56.250")) => ok("64 bytes from 192.168.56.250"),
            ("ping", _) => fail("Request timeout"),
            _ => ok(""),
        });
        assert_eq!(free_ip().await, Some("192.168.56.249"));
        let v = vagrantfile(Provider::Virtualbox, "x", false, free_ip().await, Path::new("/t"));
        assert!(v.contains("ip: \"192.168.56.249\"") && !v.contains(".250"), "{v}");
        let _all = Tools::rules(&[]);
        assert_eq!(free_ip().await, None, "every address answers");
        assert!(!vagrantfile(Provider::Virtualbox, "x", false, None, Path::new("/t")).contains("private_network"));
    }

    #[test]
    fn the_command_output_is_told_from_vagrants_own_lines() {
        assert_eq!(uname_line("warning: something\ncyberctf-uname:Linux 6.6 x86_64\nConnection to 172.20.0.5 closed.\n"), "Linux 6.6 x86_64");
        assert_eq!(uname_line("Connection to x closed.\n"), "");
        assert_eq!(uname_command(), "echo cyberctf-uname:$(uname -srm)");
        assert!(
            vagrantfile(Provider::Hyperv, "x", false, Some(VM_IPS[0]), Path::new("/t"))
                .contains("config.vm.network \"public_network\", bridge: ENV[\"ISOLOOM_HYPERV_SWITCH\"]")
        );
        assert!(!vagrantfile(Provider::Virtualbox, "x", false, Some(VM_IPS[0]), Path::new("/t")).contains("public_network"));
    }

    #[test]
    fn qemu_test_vm_falls_back_to_software_emulation_without_kvm() {
        let with_kvm = vagrantfile(Provider::Qemu, "generic/alpine319", false, Some(VM_IPS[0]), Path::new("/t"));
        assert!(with_kvm.contains("h.memory = \"1G\"") && !with_kvm.contains("accel=tcg"), "{with_kvm}");
        let without = vagrantfile(Provider::Qemu, "generic/alpine319", true, Some(VM_IPS[0]), Path::new("/t"));
        assert!(without.contains("h.machine = \"q35,accel=tcg\"") && without.contains("h.cpu = \"max\""), "{without}");
        // Other providers are unaffected.
        assert!(!vagrantfile(Provider::Virtualbox, "x", true, Some(VM_IPS[0]), Path::new("/t")).contains("accel"));
    }

    #[test]
    fn a_box_counts_only_for_its_provider_and_cpu() {
        let list = "bento/debian-12   (virtualbox, 202510.26.0, (amd64))\ngeneric/alpine319 (libvirt, 4.3.12)\n";
        assert!(box_listed(list, "bento/debian-12", "virtualbox", false));
        assert!(!box_listed(list, "bento/debian-12", "virtualbox", true));
        assert!(!box_listed(list, "bento/debian-12", "parallels", false));
        // No architecture listed: any CPU.
        assert!(box_listed(list, "generic/alpine319", "libvirt", true));
        assert!(!box_listed(list, "generic/alpine", "libvirt", true));
    }

    #[test]
    fn box_listings_with_windows_endings_wide_spacing_or_look_alike_names_match_exactly() {
        // CRLF from Vagrant on Windows, tabs or runs of spaces before the parenthesis, a name
        // that merely starts with the wanted one, and the empty listing.
        let list = "bento/debian-12\t(virtualbox, 1)\r\nbento/debian-120    (virtualbox, 1)\r\ngeneric/alpine319 (vmware_desktop, 4.3.12, (arm64))\r\n";
        assert!(box_listed(list, "bento/debian-12", "virtualbox", false));
        assert!(box_listed(list, "bento/debian-120", "virtualbox", true));
        assert!(!box_listed(list, "bento/debian-1", "virtualbox", false));
        assert!(box_listed(list, "generic/alpine319", "vmware_desktop", true));
        assert!(!box_listed(list, "generic/alpine319", "vmware_desktop", false), "an ARM box is not an x86 one");
        assert!(!box_listed(list, "generic/alpine319", "vmware", true), "the provider id is whole");
        assert!(!box_listed("There are no installed boxes! Use `vagrant box add` to add some.\n", "generic/alpine319", "qemu", false));
        assert!(!box_listed("", "generic/alpine319", "qemu", false));
    }

    #[test]
    fn the_test_vm_is_on_a_private_network_where_the_provider_has_one() {
        assert!(vagrantfile(Provider::Virtualbox, "x", false, Some(VM_IPS[0]), Path::new("/t")).contains(VM_IPS[0]));
        assert!(!vagrantfile(Provider::Utm, "x", false, Some(VM_IPS[0]), Path::new("/t")).contains(VM_IPS[0]));
        assert_eq!(ping_args("10.0.0.1")[4], "10.0.0.1");
    }

    /// The Windows uninstaller's removal of the test VM (packaging/windows): the app's own, for the
    /// test's folder in the app's cache (`%LOCALAPPDATA%\<identifier>\selftest\vm`). Written by
    /// this test; a change shows in the diff, and CI fails on one left uncommitted.
    #[test]
    fn the_uninstaller_removes_the_test_vm_as_the_app_does() {
        let under = format!("\"$env:LOCALAPPDATA\\{}\\selftest\\vm\\\"", crate::platform::config::KEYCHAIN_SERVICE);
        let script = format!(
            "# Written by `cargo test` (machine/selftest/vm.rs) from the app's own removal: don't edit.\n# Run by installer-hooks.nsh before the uninstaller removes the app's folders.\n{}",
            crate::platform::exec::leftover_script(VM_NAME, &under)
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/windows/remove-test-vm.ps1");
        let crlf = script.replace('\n', "\r\n");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(crlf.as_str()) {
            std::fs::write(&path, &crlf).unwrap();
        }
        assert!(script.contains(r#"StartsWith("$env:LOCALAPPDATA\org.cyberctf.desktop\selftest\vm\", "#), "{script}");
    }

    #[test]
    fn a_virtualbox_test_vm_carries_its_folder_so_a_leftover_is_known_as_the_tests() {
        let v = vagrantfile(Provider::Virtualbox, "x", false, None, Path::new(r"C:\Users\O'Brien\selftest\vm"));
        assert!(v.contains(r#"h.customize ["modifyvm", :id, "--description", 'cyberctf-selftest:C:\\Users\\O\'Brien\\selftest\\vm']"#), "{v}");
    }

    #[tokio::test]
    async fn an_earlier_tests_vm_is_removed_before_the_test_and_a_foreign_one_stops_it() {
        // Regression: a VM left running that Vagrant no longer knew made every later test fail at
        // `vagrant up`; the destroy's failure was dropped and nothing removed the VM.
        use crate::platform::exec::fake::{Tools, fail, ok};
        let dir = std::env::temp_dir().join(format!("cyberctf-selftest-vm-{}", rand::random::<u32>()));
        std::fs::create_dir_all(dir.join(".vagrant").join("machines").join("default").join("virtualbox")).unwrap();
        let ours = format!("description=\"{}\"\n", leftover::vbox_description(&dir));
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = events.clone();
        let r = Reporter::new(move |e: crate::runtime::selftest_report::Event| seen.lock().unwrap().push(format!("{}:{}", e.step, e.state)));
        let tools = Tools::rules(&[
            ("vagrant destroy", fail("The machine isn't created")),
            ("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")),
            ("VBoxManage showvminfo abc", ok(&ours)),
            ("vagrant --version", fail("stop here")),
        ]);
        crate::testing::logs::take_warnings();
        let _ = run_test(&dir, None, &r).await;
        assert!(tools.called("VBoxManage unregistervm abc --delete"), "{:?}", tools.calls());
        assert!(!dir.join(".vagrant").exists(), "Vagrant's stale state is gone");
        assert!(tools.called("vagrant --version"), "the test went on: {:?}", tools.calls());
        assert_eq!(events.lock().unwrap()[0], "leftover:ok");
        assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("The machine isn't created")), "the destroy's failure is logged");

        let tools = Tools::rules(&[("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")), ("VBoxManage showvminfo abc", ok("description=\"\"\n"))]);
        events.lock().unwrap().clear();
        let err = run_test(&dir, None, &r).await.unwrap_err();
        assert!(err.to_string().contains("isn't this test's"), "{err}");
        assert!(!tools.called("unregistervm") && !tools.called("vagrant --version"), "{:?}", tools.calls());
        assert_eq!(*events.lock().unwrap(), ["leftover:fail"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_provider_gets_its_own_vm_settings() {
        for (p, needle) in [
            (Provider::Virtualbox, "h.name = \"CyberCTF test VM\""),
            (Provider::VmwareDesktop, "h.vmx[\"displayName\"]"),
            (Provider::Parallels, "h.name = "),
            (Provider::Libvirt, "h.default_prefix = \"cyberctf-\""),
            (Provider::Hyperv, "h.vmname = "),
            (Provider::Utm, "h.name = "),
        ] {
            let v = vagrantfile(p, "box/x", false, Some(VM_IPS[0]), Path::new("/t"));
            assert!(v.contains(needle) && v.contains(&format!("config.vm.provider \"{}\"", p.id())), "{v}");
            assert!(v.starts_with("Vagrant.configure") && v.ends_with("end\n"));
        }
    }

    #[test]
    fn probes_and_boxes_per_provider() {
        assert_eq!(responds(Provider::Virtualbox).map(|r| r.0), Some("VBoxManage"));
        assert_eq!(responds(Provider::VmwareDesktop).map(|r| r.0), Some("vmrun"));
        assert_eq!(responds(Provider::Parallels).map(|r| r.0), Some("prlctl"));
        assert_eq!(responds(Provider::Libvirt).map(|r| r.0), Some("virsh"));
        assert!(responds(Provider::Utm).is_some_and(|r| r.0.ends_with("utmctl")));
        assert!(responds(Provider::Qemu).is_none());
        assert_eq!(candidate_boxes(Provider::Virtualbox, false), ["generic/alpine319", "bento/debian-12"]);
        assert_eq!(candidate_boxes(Provider::Parallels, true), ["bento/debian-12"]);
        assert_eq!(candidate_boxes(Provider::Utm, true), ["utm/bookworm"]);
        assert_eq!(candidate_boxes(Provider::Qemu, true), ["generic/alpine319"]);
        assert!(has_private_network(Provider::Libvirt) && !has_private_network(Provider::Qemu) && !has_private_network(Provider::Hyperv));
        // QEMU stores and lists its box as libvirt (vagrant-qemu boots a libvirt box); others use their own id.
        assert_eq!(box_provider(Provider::Qemu), "libvirt");
        assert_eq!(box_provider(Provider::Virtualbox), "virtualbox");
        assert_eq!(box_provider(Provider::Libvirt), "libvirt");
    }
}
