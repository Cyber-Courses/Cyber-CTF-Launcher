use super::*;

/// [`unavailable_reason`] with the needs [`detect_one`] reads for `detection`.
fn reason_of(provider: Provider, vagrant_installed: bool, detection: Detection<'_>) -> Option<Unavailable> {
    unavailable_reason(provider, &needs(vagrant_installed, detection.plugin_installed, detection.utility), detection)
}

/// The plugin names alone, as the checks before versions mattered read them.
fn parse_plugins(out: &str) -> Vec<String> {
    parse_plugin_versions(out).into_iter().map(|(name, _)| name).collect()
}

#[test]
fn the_preferred_hypervisor_is_picked_when_ready_else_the_first_ready_one() {
    let ready = [Provider::Virtualbox, Provider::Qemu];
    assert_eq!(pick(Some(Provider::Qemu), &ready), Some(Provider::Qemu));
    assert_eq!(pick(Some(Provider::Parallels), &ready), Some(Provider::Virtualbox), "not ready: the first ready one");
    assert_eq!(pick(None, &ready), Some(Provider::Virtualbox));
    assert_eq!(pick(Some(Provider::Qemu), &[]), None);
}

#[test]
fn vmwares_desktop_hypervisor_is_named_for_the_os_and_nothing_else_is() {
    assert_eq!(Provider::VmwareDesktop.label_on("macos"), "VMware Fusion");
    assert_eq!(Provider::VmwareDesktop.label_on("windows"), "VMware Workstation");
    assert_eq!(Provider::VmwareDesktop.label_on("linux"), "VMware Workstation");
    assert_eq!(Provider::VmwareDesktop.label_on("freebsd"), "VMware");
    for p in Provider::ALL.iter().chain(&Provider::CLOUDS).filter(|p| **p != Provider::VmwareDesktop) {
        for os in ["macos", "linux", "windows"] {
            assert_eq!(p.label_on(os), p.label(), "{p:?} on {os}");
        }
    }
}

#[test]
fn parses_vagrant_plugin_list() {
    let out = "vagrant-libvirt (0.12.2, global)\nvagrant_utm (0.1.3, global)\nvagrant-vmware-esxi (2.5.5, global)\n  - Version Constraint: > 0\n";
    assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm", "vagrant-vmware-esxi"]);
}

#[test]
fn plugin_versions_are_read_and_a_missing_one_is_none() {
    let out = "vagrant-libvirt (0.12.2, global)\nvagrant_utm (0.2.0, global)\n  - Version Constraint: > 0\nvagrant-vmware-esxi\nvagrant-qemu (0.7.0)\n";
    assert_eq!(
        parse_plugin_versions(out),
        [
            ("vagrant-libvirt".to_string(), Some("0.12.2".to_string())),
            ("vagrant_utm".to_string(), Some("0.2.0".to_string())),
            ("vagrant-vmware-esxi".to_string(), None),
            ("vagrant-qemu".to_string(), Some("0.7.0".to_string())),
        ]
    );
}

#[test]
fn versions_compare_number_by_number() {
    assert!(version_at_least("0.2.0", "0.2.0"));
    assert!(version_at_least("0.10.0", "0.2.0"), "10 > 2, not \"1\" < \"2\"");
    assert!(version_at_least("4.7", "4.6.5"), "a missing part reads as 0");
    assert!(version_at_least("5.0.6", "4.6.5"));
    assert!(!version_at_least("0.1.6", "0.2.0"));
    assert!(!version_at_least("4.5.1", "4.6.5"));
    assert!(!version_at_least("0.2.0.beta", "0.2.0.1"), "a word reads as 0");
    assert!(version_at_least("", "0") && !version_at_least("", "0.1"));
}

#[test]
fn a_plugin_listing_that_failed_says_so_rather_than_plugin_missing() {
    let detection = |plugins_unknown| Detection {
        unresponsive: false,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version: None,
        plugin: Some("vagrant-libvirt"),
        plugin_installed: false,
        plugin_version: None,
        plugins_unknown,
        utility: None,
        account: None,
    };
    let needs = [Need::Plugin];
    assert_eq!(
        unavailable_reason(Provider::Libvirt, &needs, detection(Some("exited with code 1"))),
        Some(Unavailable::PluginsUnknown { plugin: "vagrant-libvirt".into(), reason: "exited with code 1".into() })
    );
    assert_eq!(unavailable_reason(Provider::Libvirt, &needs, detection(None)), Some(Unavailable::PluginMissing { plugin: "vagrant-libvirt".into() }));
}

#[test]
fn an_old_utm_plugin_or_utm_reads_as_too_old_and_says_what_to_do() {
    let detection = |hypervisor_version, plugin_version, plugin_installed| Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version,
        plugin: Some("vagrant_utm"),
        plugin_installed,
        plugin_version,
        utility: None,
        account: None,
    };
    // The plugin the failure was seen with: setup installs it again, which updates it.
    assert_eq!(plugin_too_old(Provider::Utm, Some("0.1.6")), Some(("0.1.6", "0.2.0")));
    assert_eq!(
        reason_of(Provider::Utm, true, detection(Some("4.7.5"), Some("0.1.6"), false)),
        Some(Unavailable::PluginTooOld { plugin: "vagrant_utm".into(), version: "0.1.6".into(), min: "0.2.0".into() })
    );
    // A UTM the plugin refuses, said before the plugin (updating the plugin wouldn't help).
    assert_eq!(
        reason_of(Provider::Utm, true, detection(Some("4.5.1"), Some("0.1.6"), false)),
        Some(Unavailable::HypervisorTooOld { name: "UTM".into(), version: "4.5.1".into(), min: "4.6.5".into() })
    );
    // Recent enough, or no version to judge: nothing stands in the way.
    assert_eq!(reason_of(Provider::Utm, true, detection(Some("4.7.5"), Some("0.2.0"), true)), None);
    assert_eq!(reason_of(Provider::Utm, true, detection(None, None, true)), None);
    assert_eq!(plugin_too_old(Provider::Utm, None), None);
    // Other providers aren't held to a version.
    assert_eq!(plugin_too_old(Provider::Qemu, Some("0.1.0")), None);
    assert_eq!(hypervisor_too_old(Provider::Virtualbox, Some("6.0")), None);
}

#[test]
fn provider_ids_match_serde_names() {
    for p in Provider::ALL {
        assert_eq!(serde_json::to_value(p).unwrap(), p.id());
    }
}

#[test]
fn parse_plugins_handles_blank_versions_and_indentation() {
    // Built-in-only output, trailing detail lines, blank lines, and non-plugin noise.
    let out = "\nvagrant-vmware-desktop (3.0.4, global)\n  - Version Constraint: > 0\nvagrant-qemu (0.3.3)\nsomething-else (1.0.0)\n\n";
    assert_eq!(parse_plugins(out), vec!["vagrant-vmware-desktop", "vagrant-qemu"]);
    assert_eq!(parse_plugins(""), Vec::<String>::new());
    assert_eq!(parse_plugins("No plugins installed.\n"), Vec::<String>::new());
}

#[test]
fn plugin_lists_with_windows_line_ends_indentation_and_broken_lines_still_name_the_plugins() {
    // CRLF (Windows), an indented entry, a line that is only a version in brackets (a wrapped
    // or cut line), and an entry with no version at all: the plugins are still found, and
    // no name carries a `\r` that would then never match.
    let out =
        "vagrant-libvirt (0.12.2, global)\r\n  vagrant_utm (0.1.3, global)\r\n(2.5.5, global)\r\n  - Version Constraint: > 0\r\nvagrant-vmware-esxi\r\n\r\n";
    assert_eq!(parse_plugins(out), ["vagrant-libvirt", "vagrant_utm", "vagrant-vmware-esxi"]);
    // A version glued to the name is not how Vagrant prints, and is not a known plugin.
    assert_eq!(parse_plugins("vagrant-qemu(0.3.3)\n"), ["vagrant-qemu(0.3.3)"]);
}

#[test]
fn parse_plugins_accepts_both_hyphen_and_underscore_prefixes() {
    let out = "vagrant-libvirt (0.12.2)\nvagrant_utm (0.1.3)\n";
    assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm"]);
}

#[test]
fn all_detectable_providers_exclude_clouds() {
    // ALL is the locally detectable catalogue; cloud and AWS-family are never detected.
    for p in Provider::ALL {
        assert!(!matches!(p, Provider::Aws | Provider::Azure | Provider::Gcp | Provider::DigitalOcean | Provider::Linode | Provider::Oci));
    }
    assert_eq!(Provider::ALL.len(), 9);
}

#[test]
fn remote_and_cloud_classification() {
    // Servers are remote but not cloud; the billed accounts are both.
    for server in Provider::SERVERS {
        assert!(server.is_remote() && !server.is_cloud());
    }
    for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
        assert!(cloud.is_remote() && cloud.is_cloud());
    }
    // Local hypervisors are neither.
    for local in [Provider::Virtualbox, Provider::Hyperv, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm] {
        assert!(!local.is_remote() && !local.is_cloud());
    }
}

#[test]
fn built_in_providers_need_no_plugin_and_clouds_have_none() {
    assert_eq!(Provider::Virtualbox.plugin(), None);
    assert_eq!(Provider::Hyperv.plugin(), None);
    // Each non-builtin local/remote-hypervisor provider names a plugin.
    for p in [Provider::VmwareDesktop, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox] {
        assert!(p.plugin().is_some(), "{p:?}");
    }
    // Cloud providers are not Vagrant providers, so no plugin.
    for p in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
        assert_eq!(p.plugin(), None, "{p:?}");
    }
}

#[test]
fn ids_are_unique_and_round_trip_through_serde() {
    let all = [
        Provider::Virtualbox,
        Provider::VmwareDesktop,
        Provider::Hyperv,
        Provider::Parallels,
        Provider::Libvirt,
        Provider::Qemu,
        Provider::Utm,
        Provider::VmwareEsxi,
        Provider::Proxmox,
        Provider::Aws,
        Provider::Azure,
        Provider::Gcp,
        Provider::DigitalOcean,
        Provider::Linode,
        Provider::Oci,
    ];
    let mut ids: Vec<&str> = all.iter().map(|p| p.id()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "provider ids must be unique");
    // id() is the serde wire name, and it round-trips back to the same provider.
    for p in all {
        let round: Provider = serde_json::from_value(serde_json::Value::String(p.id().to_string())).unwrap();
        assert_eq!(round, p);
    }
}

#[test]
fn applicability_follows_the_os() {
    use std::env::consts::OS;
    assert_eq!(Provider::Hyperv.applicable(), OS == "windows");
    assert_eq!(Provider::Parallels.applicable(), OS == "macos");
    assert_eq!(Provider::Utm.applicable(), OS == "macos");
    assert_eq!(Provider::Libvirt.applicable(), OS == "linux");
    for remote in [Provider::VmwareEsxi, Provider::Proxmox, Provider::Qemu, Provider::Aws] {
        assert!(remote.applicable(), "{remote:?}");
    }
}

#[test]
fn a_mac_is_offered_the_hypervisor_made_for_its_cpu_and_keeps_one_installed() {
    // Was the webview's rule alone (`usableHypervisors`): the backend listed both, and offered
    // to install VirtualBox on Apple Silicon.
    assert!(!Provider::Virtualbox.offered_on("macos", "aarch64", false));
    assert!(Provider::Utm.offered_on("macos", "aarch64", false));
    assert!(Provider::Virtualbox.offered_on("macos", "x86_64", false));
    assert!(!Provider::Utm.offered_on("macos", "x86_64", false));
    assert!(Provider::Virtualbox.offered_on("macos", "aarch64", true), "installed: the player chose it");
    assert!(Provider::Virtualbox.offered_on("linux", "aarch64", false));
    assert!(!Provider::Utm.offered_on("linux", "aarch64", true), "never where it doesn't apply");
    assert!(!Provider::Hyperv.offered_on("macos", "aarch64", true));
}

#[test]
fn probes_name_the_hypervisors_tool() {
    assert_eq!(Provider::Virtualbox.probe().unwrap().0, "VBoxManage");
    assert_eq!(Provider::VmwareDesktop.probe().unwrap().0, "vmrun");
    assert_eq!(Provider::Parallels.probe().unwrap().0, "prlctl");
    assert_eq!(Provider::Libvirt.probe().unwrap().0, "virsh");
    assert!(Provider::Qemu.probe().unwrap().0.starts_with("qemu-system-"));
    assert_eq!(qemu_binary(), if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" });
    for none in [Provider::Hyperv, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox, Provider::Gcp, Provider::Oci] {
        assert!(none.probe().is_none(), "{none:?}");
    }
}

#[test]
fn kvm_problems_are_explained() {
    // Whatever this machine has, an answer is either nothing or a sentence about /dev/kvm.
    if let Some(why) = kvm_problem() {
        assert!(why.message().contains("/dev/kvm"), "{why}");
    }
}

#[tokio::test]
async fn virtualbox_reads_as_not_responding_only_when_the_apps_timeout_cut_it_short() {
    // Regression: any `VBoxManage list runningvms` failure whose text said "timed out" (the
    // tool's own words) read as VirtualBox's service being stuck.
    // The probe is asked directly: VirtualBox has no build for Windows on Arm, so `detect`
    // doesn't report it there at all (the check below), whatever the probe says.
    use crate::platform::exec::fake::{Tools, fail, ok};
    let tools = |runningvms: &'static str| {
        Tools::new(move |program, args| match (program, args) {
            ("VBoxManage", ["list", "runningvms"]) => fail(runningvms),
            ("VBoxManage", _) => ok("7.1.4\n"),
            _ => ok(""),
        })
    };
    let stuck = |runningvms: &'static str| async move {
        let _tools = tools(runningvms);
        virtualbox_stuck().await
    };
    assert!(!stuck("VBoxManage: error: the guest session timed out").await, "the tool's own words");
    assert!(stuck("timed out after 8s").await, "the app's timeout");
    // And `detect` says so wherever VirtualBox applies; where it doesn't, it says nothing.
    // The host's own KVM is no part of it (a Linux host with kvm_intel loaded would answer
    // KvmHoldsVirtualization first).
    assume_devices_ready();
    let _tools = tools("timed out after 8s");
    let virtualbox = detect(true).await.into_iter().find(|s| s.provider == Provider::Virtualbox);
    if Provider::Virtualbox.applicable() {
        assert_eq!(virtualbox.and_then(|s| s.reason), Some(Unavailable::VirtualboxStuck));
    } else {
        assert!(virtualbox.is_none(), "{:?}", virtualbox.map(|s| s.reason));
    }
}

#[test]
fn the_first_missing_layer_is_the_reason() {
    let r = |p, vagrant, stuck, hv, plugin, installed| {
        reason_of(
            p,
            vagrant,
            Detection {
                unresponsive: stuck,
                plugins_unknown: None,
                hypervisor: hv,
                mkisofs_missing: false,
                hypervisor_version: None,
                plugin,
                plugin_installed: installed,
                plugin_version: None,
                utility: None,
                account: None,
            },
        )
    };
    let hyperv = |needs_admin: bool| Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version: None,
        plugin: None,
        plugin_installed: true,
        plugin_version: None,
        utility: None,
        account: needs_admin.then_some(AccountLacks::HypervAdmin),
    };
    assert_eq!(reason_of(Provider::Hyperv, true, hyperv(true)), Some(Unavailable::HypervNeedsAdmin));
    let said = Unavailable::HypervNeedsAdmin.message();
    assert!(said.contains("Hyper-V Administrators") && said.contains("Allow my account"));
    assert_eq!(reason_of(Provider::Hyperv, true, hyperv(false)), None);
    assert_eq!(r(Provider::Virtualbox, false, true, Some(false), None, false), Some(Unavailable::VagrantMissing));
    assert_eq!(r(Provider::Virtualbox, true, true, Some(true), None, true), Some(Unavailable::VirtualboxStuck));
    assert_eq!(r(Provider::Virtualbox, true, false, Some(false), None, true), Some(Unavailable::NotInstalled { program: Some("VBoxManage".into()) }));
    assert_eq!(r(Provider::Hyperv, true, false, Some(false), None, true), Some(Unavailable::HypervOff));
    assert_eq!(r(Provider::Utm, true, false, Some(false), Some("vagrant_utm"), true), Some(Unavailable::NotInstalled { program: None }));
    assert_eq!(
        r(Provider::Parallels, true, false, Some(true), Some("vagrant-parallels"), false),
        Some(Unavailable::PluginMissing { plugin: "vagrant-parallels".into() })
    );
    assert_eq!(r(Provider::Virtualbox, true, false, Some(true), None, true), None);
    assert_eq!(r(Provider::VmwareEsxi, true, false, None, Some("vagrant-vmware-esxi"), true), None);
    // libvirt also needs a usable /dev/kvm.
    assert_eq!(r(Provider::Libvirt, true, false, Some(true), Some("vagrant-libvirt"), true), kvm_problem());
}

#[test]
fn libvirt_joined_since_logging_in_waits_for_the_next_login() {
    let libvirt = |relogin: bool| Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version: None,
        plugin: Some("vagrant-libvirt"),
        plugin_installed: true,
        plugin_version: None,
        utility: None,
        account: relogin.then_some(AccountLacks::NewLogin),
    };
    assert_eq!(reason_of(Provider::Libvirt, true, libvirt(true)), Some(Unavailable::NewLogin { group: "libvirt".into() }));
    assert_eq!(reason_of(Provider::Libvirt, true, libvirt(false)), kvm_problem());
    // Vagrant missing still comes first: the setup installs it in the same pass.
    assert_eq!(reason_of(Provider::Libvirt, false, libvirt(true)), Some(Unavailable::VagrantMissing));
}

#[test]
fn qemu_on_linux_needs_the_kvm_device_the_labs_boot_with() {
    // Regression: QEMU read as ready on Debian 13 for a player outside the kvm group
    // (/dev/kvm root:kvm 0660): the VM self-test fell back to software emulation and passed,
    // then every lab's `-machine q35,accel=kvm` failed ("Could not access KVM kernel module:
    // Permission denied").
    let qemu = |relogin: bool| Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version: None,
        plugin: Some("vagrant-qemu"),
        plugin_installed: true,
        plugin_version: None,
        utility: None,
        account: relogin.then_some(AccountLacks::NewLogin),
    };
    assert!(uses_kvm(Provider::Qemu, "linux") && !uses_kvm(Provider::Qemu, "macos") && !uses_kvm(Provider::Qemu, "windows"));
    if cfg!(target_os = "linux") {
        assert_eq!(reason_of(Provider::Qemu, true, qemu(false)), kvm_problem(), "whatever this machine's /dev/kvm says");
        assert_eq!(reason_of(Provider::Qemu, true, qemu(true)), Some(Unavailable::NewLogin { group: kvm_group() }));
    } else {
        // HVF on a Mac, WHPX on Windows: no /dev/kvm to open.
        assert_eq!(reason_of(Provider::Qemu, true, qemu(false)), None);
    }
    assert_eq!(login_groups(Provider::Qemu, "linux"), [kvm_group()]);
    assert_eq!(login_groups(Provider::Qemu, "macos"), Vec::<String>::new());
    let libvirt = login_groups(Provider::Libvirt, "linux");
    assert!(libvirt.iter().any(|g| g == "libvirt") && libvirt.contains(&kvm_group()), "{libvirt:?}");
    assert_eq!(login_groups(Provider::Virtualbox, "linux"), Vec::<String>::new());
}

#[test]
fn qemu_without_mkisofs_says_its_vms_need_it_vagrant_first() {
    // Regression: QEMU read as ready without mkisofs, and every VM-form lab failed at its first VM
    // ("The executable 'mkisofs' Vagrant is trying to run was not found in the PATH variable").
    assert!(needs_mkisofs(Provider::Qemu, "linux"));
    for (p, os) in [(Provider::Qemu, "macos"), (Provider::Qemu, "windows"), (Provider::Libvirt, "linux"), (Provider::Virtualbox, "linux")] {
        assert!(!needs_mkisofs(p, os), "{p:?} on {os}");
    }
    let qemu = Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(false),
        mkisofs_missing: true,
        hypervisor_version: None,
        plugin: Some("vagrant-qemu"),
        plugin_installed: true,
        plugin_version: None,
        utility: None,
        account: None,
    };
    assert_eq!(reason_of(Provider::Qemu, true, qemu), Some(Unavailable::MkisofsMissing));
    assert_eq!(reason_of(Provider::Qemu, false, qemu), Some(Unavailable::VagrantMissing));
    let said = Unavailable::MkisofsMissing.message();
    assert!(said.starts_with("QEMU VMs need mkisofs") && said.contains("genisoimage"), "{said}");
}

#[tokio::test]
async fn qemu_on_linux_reads_as_not_installed_without_mkisofs_which_its_install_brings() {
    // The check itself: on Linux, `qemu-system-*` alone is QEMU half installed, so the
    // Machine page and the guided setup offer its install (which adds genisoimage); elsewhere
    // mkisofs is never asked for.
    use crate::platform::exec::fake::{Tools, ok};
    let tools = Tools::new(|program, _| if program == "mkisofs" { None } else { ok("QEMU emulator version 8.2.2\n") });
    let plugins = Ok(vec![("vagrant-qemu".to_owned(), Some("0.8.0".to_owned()))]);
    let qemu = detect_one(Provider::Qemu, true, &plugins).await.unwrap();
    if cfg!(target_os = "linux") {
        assert_eq!((qemu.hypervisor, qemu.available, qemu.reason), (Some(false), false, Some(Unavailable::MkisofsMissing)));
    } else {
        assert_eq!(qemu.hypervisor, Some(true));
        assert!(!tools.called("mkisofs"), "{:?}", tools.calls());
    }
}

/// A VMware detection: Fusion or Workstation installed, the plugin and the utility as given.
fn vmware(plugin_installed: bool, utility: UtilityState) -> Detection<'static> {
    Detection {
        unresponsive: false,
        plugins_unknown: None,
        hypervisor: Some(true),
        mkisofs_missing: false,
        hypervisor_version: None,
        plugin: Some("vagrant-vmware-desktop"),
        plugin_installed,
        plugin_version: None,
        utility: Some(utility),
        account: None,
    }
}

#[test]
fn a_provider_is_ready_exactly_when_nothing_is_needed() {
    // The shared rule: readiness (`unavailable_reason`, what `available` and `ready` read) and
    // the setup steps (which install each of `needs`) agree on every combination, so a setup
    // that installs everything it lists leaves the provider ready, and one that is ready
    // lists nothing to install.
    for vagrant in [false, true] {
        for plugin_installed in [false, true] {
            for utility in [None, Some(UtilityState::Missing), Some(UtilityState::Stopped), Some(UtilityState::Running)] {
                let detection = Detection { utility, ..vmware(plugin_installed, UtilityState::Running) };
                let needs = needs(vagrant, plugin_installed, utility);
                let reason = reason_of(Provider::VmwareDesktop, vagrant, detection);
                assert_eq!(reason.is_none(), needs.is_empty(), "vagrant {vagrant}, plugin {plugin_installed}, utility {utility:?}: {reason:?} / {needs:?}");
            }
        }
    }
    assert_eq!(needs(false, false, Some(UtilityState::Missing)), [Need::Vagrant, Need::Plugin, Need::VmwareUtility], "in setup's order");
}

#[test]
fn vmware_needs_the_utility_running_as_well_as_its_plugin() {
    assert!(Provider::VmwareDesktop.needs_vmware_utility());
    assert!(Provider::ALL.iter().chain(&Provider::CLOUDS).filter(|p| **p != Provider::VmwareDesktop).all(|p| !p.needs_vmware_utility()));
    // The plugin alone isn't enough (the loop: setup put the plugin in, labs still couldn't run).
    assert_eq!(reason_of(Provider::VmwareDesktop, true, vmware(true, UtilityState::Missing)), Some(Unavailable::VmwareUtilityMissing));
    assert_eq!(reason_of(Provider::VmwareDesktop, true, vmware(true, UtilityState::Stopped)), Some(Unavailable::VmwareUtilityStopped));
    // The plugin first: it is what setup installs first.
    assert_eq!(
        reason_of(Provider::VmwareDesktop, true, vmware(false, UtilityState::Missing)),
        Some(Unavailable::PluginMissing { plugin: "vagrant-vmware-desktop".into() })
    );
    assert_eq!(reason_of(Provider::VmwareDesktop, true, vmware(true, UtilityState::Running)), None);
}

/// Fusion or Workstation installed (`vmrun` answers), Vagrant installed, with `plugins`.
fn vmware_machine(plugins: &'static str) -> crate::platform::exec::fake::Tools {
    use crate::platform::exec::fake::{Tools, fail, ok};
    Tools::new(move |program, args| match (program, args) {
        ("vagrant", ["--version"]) => ok("Vagrant 2.4.9\n"),
        ("vagrant", ["plugin", "list"]) => ok(plugins),
        ("vmrun", _) => fail("usage: vmrun [AUTHENTICATION-FLAGS] COMMAND [PARAMETERS]"),
        _ => None,
    })
}

#[tokio::test]
async fn a_vmware_machine_without_the_plugin_or_the_utility_needs_both_and_is_ready_once_both_are_in() {
    if !Provider::VmwareDesktop.applicable() {
        return; // Windows on Arm: no VMware Workstation there.
    }
    // This Mac's state when the loop was seen: Fusion installed, other plugins, neither of
    // VMware's two parts.
    let before = {
        let _tools = vmware_machine("vagrant-qemu (0.8.0, global)\nvagrant-vmware-esxi (2.5.5, global)\nvagrant_utm (0.2.0, global)\n");
        status(Provider::VmwareDesktop).await.unwrap()
    };
    assert_eq!(before.hypervisor, Some(true));
    assert_eq!(before.needs, [Need::Plugin, Need::VmwareUtility]);
    assert_eq!(before.utility, Some(UtilityState::Missing));
    assert!(!before.available);
    assert_eq!(ready(std::slice::from_ref(&before), true), Vec::<Provider>::new());

    // The plugin alone: still not ready, and what is left is said.
    let plugin_only = {
        let _tools = vmware_machine("vagrant-vmware-desktop (3.0.5, global)\n");
        status(Provider::VmwareDesktop).await.unwrap()
    };
    assert_eq!(plugin_only.needs, [Need::VmwareUtility]);
    assert_eq!(plugin_only.reason, Some(Unavailable::VmwareUtilityMissing));
    assert_eq!(ready(std::slice::from_ref(&plugin_only), true), Vec::<Provider>::new());

    // Both in: ready, nothing left to install.
    let after = {
        let _tools = vmware_machine("vagrant-vmware-desktop (3.0.5, global)\n");
        let _utility = fake_utility::Utility::set(UtilityState::Running);
        status(Provider::VmwareDesktop).await.unwrap()
    };
    assert!(after.needs.is_empty() && after.available, "{:?}", after.reason);
    assert_eq!(ready(std::slice::from_ref(&after), true), [Provider::VmwareDesktop]);
}

#[test]
fn statuses_serialize_in_camel_case() {
    let s = ProviderStatus {
        provider: Provider::Virtualbox,
        remote: false,
        available: true,
        hypervisor: Some(true),
        plugin: None,
        plugin_installed: true,
        utility: None,
        needs: vec![Need::Plugin, Need::VmwareUtility],
        reason: Some(Unavailable::PluginMissing { plugin: "vagrant-libvirt".into() }),
        account_lacks: Some(AccountLacks::HypervAdmin),
    };
    let v = serde_json::to_value(s).unwrap();
    assert_eq!(v["provider"], "virtualbox");
    assert_eq!(v["reason"], serde_json::json!({ "code": "pluginMissing", "args": { "plugin": "vagrant-libvirt" } }));
    assert_eq!(v["accountLacks"], "hypervAdmin");
    assert_eq!(v["pluginInstalled"], true);
    assert_eq!(v["needs"], serde_json::json!(["plugin", "vmwareUtility"]));
}
