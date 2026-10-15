//! The VM self-test: a real throwaway VM on a local hypervisor.

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

/// The local provider the VM test uses: the preferred one (Settings) when it's ready,
/// else the first one whose hypervisor and plugin are ready.
pub(super) async fn test_provider(preferred: Option<Provider>) -> Option<Provider> {
    let ready: Vec<Provider> =
        providers::detect(true).await.iter().filter(|s| !s.remote && s.available && s.hypervisor != Some(false)).map(|s| s.provider).collect();
    preferred.filter(|p| ready.contains(p)).or_else(|| ready.first().copied())
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

/// The local hypervisors the VM self-test can use.
pub(super) const TEST_PROVIDERS: [Provider; 7] =
    [Provider::Virtualbox, Provider::VmwareDesktop, Provider::Parallels, Provider::Utm, Provider::Libvirt, Provider::Qemu, Provider::Hyperv];

/// Small public boxes that exist for this provider and CPU, smallest first.
pub(crate) fn candidate_boxes(p: Provider, arm: bool) -> &'static [&'static str] {
    match (p, arm) {
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, false) => &["generic/alpine319", "bento/debian-12"],
        (Provider::Virtualbox | Provider::VmwareDesktop | Provider::Parallels, true) => &["bento/debian-12"],
        (Provider::Utm, _) => &["utm/bookworm"],
        _ => &["generic/alpine319"],
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
    match candidates.iter().find(|c| box_listed(&list, c, p.id(), arm)) {
        Some(c) => (c, true),
        None => (candidates[0], false),
    }
}

/// Whether `vagrant box list` has `name` for `provider` and this CPU (a box with no
/// architecture in the listing counts for any).
fn box_listed(list: &str, name: &str, provider: &str, arm: bool) -> bool {
    let arch = if arm { "arm64" } else { "amd64" };
    // Lines look like: `bento/debian-12   (virtualbox, 202510.26.0, (amd64))`
    list.lines().any(|l| {
        let mut parts = l.splitn(2, char::is_whitespace);
        let listed = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        listed == name
            && rest.contains(&format!("({provider},"))
            && (!rest.contains("(amd64)") && !rest.contains("(arm64)") || rest.contains(&format!("({arch})")))
    })
}

/// `no_kvm`: a Linux host without a usable /dev/kvm. vagrant-qemu asks for `accel=kvm` with
/// `cpu=host` there, which QEMU refuses; the test VM falls back to software emulation (slow,
/// but it boots and the check says something true).
/// `ip`: the VM's address on a private network, when it gets one.
fn vagrantfile(p: Provider, bx: &str, no_kvm: bool, ip: Option<&str>) -> String {
    let mut v = format!(
        "Vagrant.configure(\"2\") do |config|\n  config.vm.box = \"{bx}\"\n  config.vm.hostname = \"cyberctf-selftest\"\n  config.vm.boot_timeout = 600\n  config.vm.synced_folder \".\", \"/vagrant\", disabled: true\n"
    );
    if let Some(ip) = ip.filter(|_| has_private_network(p)) {
        v.push_str(&format!("  config.vm.network \"private_network\", ip: \"{ip}\"\n"));
    }
    // With several virtual switches and none named, Vagrant would stop to ask which one.
    if p == Provider::Hyperv {
        v.push_str(&crate::platform::exec::hyperv_switch_line("  ", "config"));
    }
    // A readable name in the hypervisor's VM list instead of Vagrant's `<dir>_default_<timestamp>`.
    let settings = match p {
        Provider::Virtualbox => format!("    h.name = \"{VM_NAME}\"\n    h.memory = 512\n"),
        Provider::VmwareDesktop => format!("    h.vmx[\"displayName\"] = \"{VM_NAME}\"\n    h.memory = 512\n"),
        Provider::Parallels => format!("    h.name = \"{VM_NAME}\"\n    h.memory = 512\n"),
        Provider::Libvirt => "    h.default_prefix = \"cyberctf-\"\n    h.memory = 512\n".to_string(),
        Provider::Hyperv => format!("    h.vmname = \"{VM_NAME}\"\n"),
        Provider::Utm => format!("    h.name = \"{VM_NAME}\"\n"),
        // vagrant-qemu's default is 4G, much more than a test VM needs.
        Provider::Qemu if no_kvm => "    h.memory = \"1G\"\n    h.machine = \"q35,accel=tcg\"\n    h.cpu = \"max\"\n".to_string(),
        Provider::Qemu => "    h.memory = \"1G\"\n".to_string(),
        _ => String::new(),
    };
    if !settings.is_empty() {
        v.push_str(&format!("  config.vm.provider \"{}\" do |h|\n{settings}  end\n", p.id()));
    }
    v.push_str("end\n");
    v
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
    // A test interrupted last time (window closed) may have left its VM behind.
    if dir.join(".vagrant").exists() {
        let _ = destroy(dir).await;
    }
    let res = steps(dir, preferred, r).await;
    // Destroy the VM whatever happened; the box stays cached for the next test.
    if dir.join("Vagrantfile").exists() {
        let down = destroy(dir).await;
        r.cleanup("Delete the test VM", &res, down, Some("test box kept for next time".into()));
    }
    res
}

async fn steps(dir: &Path, preferred: Option<Provider>, r: &Reporter) -> Result<()> {
    let provider = host_provider(preferred, r).await?;
    let arm = arm();
    let (bx, _) = pick_box(provider, arm).await;
    r.step("box", "Get a small test VM image", async {
        let had = ensure_box(dir, provider, bx, arm, |l| r.progress("box", "Get a small test VM image", l)).await?;
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
        std::fs::write(dir.join("Vagrantfile"), vagrantfile(provider, bx, no_kvm, ip))?;
        let steps = [Step::new(&["up", "--provider", provider.id()]), Step::new(&["ssh", "-c", &uname_command()])];
        let vagrant = Vagrant::new(dir, &[], provider == Provider::Hyperv);
        let batch = vagrant.batch(&steps, |e| {
            if let Event::Line(0, l) = e {
                r.progress("boot", "Boot the test VM", l)
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
            r.send("network", "VM reachable on a lab network", "skip", Some(format!("every test address ({}…) is taken on this machine", VM_IPS[0])))
        }
        None => r.send("network", "VM reachable on a lab network", "skip", Some(format!("{} has no private networks", provider.id()))),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let v = vagrantfile(Provider::Virtualbox, "x", false, free_ip().await);
        assert!(v.contains("ip: \"192.168.56.249\"") && !v.contains(".250"), "{v}");
        let _all = Tools::rules(&[]);
        assert_eq!(free_ip().await, None, "every address answers");
        assert!(!vagrantfile(Provider::Virtualbox, "x", false, None).contains("private_network"));
    }

    #[test]
    fn the_command_output_is_told_from_vagrants_own_lines() {
        assert_eq!(uname_line("warning: something\ncyberctf-uname:Linux 6.6 x86_64\nConnection to 172.20.0.5 closed.\n"), "Linux 6.6 x86_64");
        assert_eq!(uname_line("Connection to x closed.\n"), "");
        assert_eq!(uname_command(), "echo cyberctf-uname:$(uname -srm)");
        assert!(
            vagrantfile(Provider::Hyperv, "x", false, Some(VM_IPS[0])).contains("config.vm.network \"public_network\", bridge: ENV[\"ISOLOOM_HYPERV_SWITCH\"]")
        );
        assert!(!vagrantfile(Provider::Virtualbox, "x", false, Some(VM_IPS[0])).contains("public_network"));
    }

    #[test]
    fn qemu_test_vm_falls_back_to_software_emulation_without_kvm() {
        let with_kvm = vagrantfile(Provider::Qemu, "generic/alpine319", false, Some(VM_IPS[0]));
        assert!(with_kvm.contains("h.memory = \"1G\"") && !with_kvm.contains("accel=tcg"), "{with_kvm}");
        let without = vagrantfile(Provider::Qemu, "generic/alpine319", true, Some(VM_IPS[0]));
        assert!(without.contains("h.machine = \"q35,accel=tcg\"") && without.contains("h.cpu = \"max\""), "{without}");
        // Other providers are unaffected.
        assert!(!vagrantfile(Provider::Virtualbox, "x", true, Some(VM_IPS[0])).contains("accel"));
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
        assert!(vagrantfile(Provider::Virtualbox, "x", false, Some(VM_IPS[0])).contains(VM_IPS[0]));
        assert!(!vagrantfile(Provider::Utm, "x", false, Some(VM_IPS[0])).contains(VM_IPS[0]));
        assert_eq!(ping_args("10.0.0.1")[4], "10.0.0.1");
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
            let v = vagrantfile(p, "box/x", false, Some(VM_IPS[0]));
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
    }
}
