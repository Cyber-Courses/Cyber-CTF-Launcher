//! What the guided setup leaves on a machine on purpose, item by item, in the leftover audit's
//! own words (tests/audit/setup-leaves.tsv: the audit's kinds, globs on the names its report
//! shows), so each `installed:` record the headless setup entry writes carries the allowances a
//! caller's audit needs (`leaves`), and nobody keeps a table of their own.

use std::path::Path;

use super::super::support::results::Leaves;

/// The table, compiled in: a rename there is a test failure here.
const TABLE: &str = include_str!("../../../../tests/audit/setup-leaves.tsv");

/// One row of the table.
struct Row<'a> {
    item: &'a str,
    os: &'a str,
    kind: &'a str,
    name: &'a str,
}

/// The table's rows, comments and blank lines left out.
fn rows() -> impl Iterator<Item = Row<'static>> {
    TABLE.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| {
        let mut f = l.split('\t');
        let mut next = || f.next().unwrap_or_else(|| panic!("setup-leaves.tsv: a row short of its five columns: {l}"));
        Row { item: next(), os: next(), kind: next(), name: next() }
    })
}

/// Whether `text` matches `pattern`, a glob with `*` (anything) and `?` (one character).
pub(crate) fn glob_matches(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    go(&p, &t)
}

/// What installing `item` on `os` (`std::env::consts::OS`) leaves, by the table, then the setup
/// entry's own folder (`root`, its real path: the audit names a temp folder by its path).
pub(crate) fn of(item: &str, os: &str, root: &Path) -> Vec<Leaves> {
    let mut out: Vec<Leaves> = rows()
        .filter(|r| glob_matches(r.item, item) && (r.os == "any" || r.os == os))
        .map(|r| Leaves { kind: r.kind.to_owned(), name: r.name.to_owned() })
        .collect();
    out.push(Leaves { kind: "temp-folder".to_owned(), name: root.display().to_string() });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The audit's kinds (tests/audit/kinds.ts): what a `leaves` row may name.
    fn audit_kinds() -> Vec<String> {
        include_str!("../../../../tests/audit/kinds.ts")
            .lines()
            .filter_map(|l| Some(l.trim().strip_prefix("{ kind: \"").or_else(|| l.trim().strip_prefix("kind: \""))?.split('"').next()?.to_owned()))
            .collect()
    }

    fn names(item: &str, os: &str) -> Vec<(String, String)> {
        of(item, os, Path::new("/tmp/cyberctf-guided-setup")).into_iter().map(|l| (l.kind, l.name)).collect()
    }
    fn pair(kind: &str, name: &str) -> (String, String) {
        (kind.to_owned(), name.to_owned())
    }
    const FOLDER: (&str, &str) = ("temp-folder", "/tmp/cyberctf-guided-setup");

    #[test]
    fn every_row_names_a_kind_the_audit_reads_on_that_system() {
        let kinds = audit_kinds();
        assert!(kinds.len() > 30, "kinds.ts read: {kinds:?}");
        let mut count = 0;
        for r in rows() {
            count += 1;
            assert!(kinds.iter().any(|k| k == r.kind), "{}: {} isn't a kind of tests/audit/kinds.ts", r.item, r.kind);
            assert!(matches!(r.os, "linux" | "macos" | "windows" | "any"), "{}: os {}", r.item, r.os);
            assert!(!r.name.is_empty() && !r.item.is_empty());
        }
        assert!(count > 40);
    }

    #[test]
    fn globs_match_as_the_audits_allowlist_does() {
        assert!(glob_matches("*DOCKER*", "filter -A FORWARD -j DOCKER-USER"));
        assert!(glob_matches("* docker0 *", "nat -A POSTROUTING -s 172.17.0.0/16 ! -o docker0 -j MASQUERADE"));
        assert!(glob_matches("nat :* ACCEPT", "nat :POSTROUTING ACCEPT"));
        assert!(!glob_matches("nat :* ACCEPT", "filter :FORWARD DROP"));
        assert!(glob_matches("172.17.0.0/16 dev docker0", "172.17.0.0/16 dev docker0"));
        assert!(!glob_matches("172.17.0.0/16 dev docker0", "172.17.0.0/16 dev docker0 linkdown"));
        assert!(glob_matches("plugin:*", "plugin:vagrant-libvirt"));
        assert!(glob_matches("lab:?ello", "lab:hello") && !glob_matches("lab:?ello", "lab:hhello"));
    }

    #[test]
    fn the_docker_path_leaves_the_engines_bridge_and_firewall_on_linux_and_docker_desktop_elsewhere() {
        let linux = names("docker", "linux");
        for (kind, name) in [
            ("route", "172.17.0.0/16 dev docker0"),
            ("iptables-rule", "*DOCKER*"),
            ("iptables-rule", "filter :FORWARD DROP"),
            ("nft-table", "table ip nat"),
            ("nft-table", "table * docker*"),
            // Docker Engine 29's raw table (ubuntu2404-arm64-bare's setup-docker audit).
            ("nft-table", "table ip raw"),
            ("iptables-rule", "raw :PREROUTING ACCEPT"),
            ("iptables-rule", "raw :OUTPUT ACCEPT"),
            // Kali's containerd keeps the cri plugin on: its streaming server's localhost port
            // (kali-bare's setup-docker audit, 2026-11-12).
            ("listening-port", "tcp 127.0.0.1:* (containerd, *"),
            FOLDER,
        ] {
            assert!(linux.contains(&pair(kind, name)), "docker on linux lacks {kind} {name}: {linux:?}");
        }
        assert!(linux.iter().all(|(k, _)| k != "process"), "no Docker Desktop on Linux");
        for os in ["macos", "windows"] {
            for item in ["docker", "engine:docker-desktop"] {
                let leaves = names(item, os);
                assert!(
                    leaves.contains(&pair("process", "* com.docker.*")) && leaves.contains(&pair("listening-port", "*docker*")),
                    "{item} on {os}: {leaves:?}"
                );
                assert!(leaves.iter().all(|(k, _)| k != "route" && k != "iptables-rule"), "{item} on {os}: {leaves:?}");
            }
        }
    }

    #[test]
    fn the_docker_path_on_windows_leaves_wsl_2_hyper_vs_own_and_the_engines_hosts_entries() {
        // Docker Desktop's WSL 2 VM runs under Hyper-V's worker process, and the engine writes its
        // hosts entries (the Docker path's audit on a bare Windows clone, 2026-11-12).
        for item in ["docker", "engine:docker-desktop"] {
            let windows = names(item, "windows");
            for (kind, name) in [("process", "* \"*\\vmwp.exe\" *"), ("hosts-entry", "* *.docker.internal")] {
                assert!(windows.contains(&pair(kind, name)), "{item} on windows lacks {kind} {name}: {windows:?}");
            }
        }
        // WSL 2, which the Docker path turns on first on Windows.
        assert!(names("wsl", "windows").contains(&pair("hyperv-switch", "WSL*")));
        // Its firewall rule, by its whole name (rig windows-bare: the docker path's audit read it
        // as left once WSL 2 was installed), then Hyper-V's own: WSL 2's install on Windows 11
        // turns Hyper-V's platform on too, and the Docker path's audit on a bare clone read the
        // Default Switch, its routes, Hyper-V's 16 rules, vmms's port 2179 and NetBIOS on the
        // new vEthernet adapters as left (rig windows-bare setup-docker, 2026-11-12).
        let wsl = names("wsl", "windows");
        let rules: Vec<_> = wsl.iter().filter(|(k, _)| k == "firewall-rule").cloned().collect();
        assert_eq!(rules[0], pair("firewall-rule", "Windows Subsystem for Linux"));
        let hyperv_rules: Vec<_> = names("hyperv", "windows").into_iter().filter(|(k, _)| k == "firewall-rule").collect();
        assert_eq!(rules[1..], hyperv_rules[..], "WSL 2 leaves Hyper-V's rules too");
        for (kind, name) in [
            ("hyperv-switch", "Default Switch"),
            ("route", "* vEthernet (Default Switch)"),
            ("listening-port", ":::2179 (vmms, *)"),
            ("listening-port", "*:139 (System, *"),
        ] {
            assert!(wsl.contains(&pair(kind, name)), "wsl on windows lacks {kind} {name}: {wsl:?}");
        }
        assert_eq!(names("wsl", "linux"), vec![pair(FOLDER.0, FOLDER.1)]);
    }

    #[test]
    fn the_libvirt_and_qemu_paths_leave_libvirts_default_network_pool_bridge_and_chains() {
        for item in ["libvirt", "qemu"] {
            let leaves = names(item, "linux");
            for (kind, name) in [
                ("route", "192.168.122.0/24 dev virbr0"),
                ("libvirt-network", "default"),
                ("libvirt-pool", "default"),
                ("iptables-rule", "*LIBVIRT*"),
                ("nft-table", "table * libvirt*"),
                // iptables-nft's tables and base chains for libvirt's default network
                // (debian13-bare's setup-libvirt audit), each named, no glob.
                ("nft-table", "table ip filter"),
                ("nft-table", "table ip6 nat"),
                ("iptables-rule", "mangle :POSTROUTING ACCEPT"),
                ("iptables-rule", "filter :FORWARD ACCEPT"),
                ("iptables-rule", "nat :PREROUTING ACCEPT"),
                // The default network's dnsmasq, which Ubuntu starts at install (ubuntu2404-bare's
                // setup-libvirt audit), on virbr0's address and interface only.
                ("listening-port", "udp 192.168.122.1:53 (dnsmasq, *)"),
                ("listening-port", "tcp 192.168.122.1:53 (dnsmasq, *)"),
                ("listening-port", "udp 0.0.0.0%virbr0:67 (dnsmasq, *)"),
                FOLDER,
            ] {
                assert!(leaves.contains(&pair(kind, name)), "{item} lacks {kind} {name}: {leaves:?}");
            }
            assert_eq!(names(item, "macos"), vec![pair(FOLDER.0, FOLDER.1)], "{item} leaves nothing of libvirt's on a Mac");
        }
        // The self-test's first VM makes vagrant-libvirt's management network.
        let selftest = names("selftest:vm", "linux");
        assert!(
            selftest.contains(&pair("libvirt-network", "vagrant-libvirt")) && selftest.contains(&pair("route", "192.168.121.0/24 dev virbr*")),
            "{selftest:?}"
        );
    }

    #[test]
    fn the_hyperv_path_leaves_the_default_switch_and_its_nat_route() {
        let leaves = names("hyperv", "windows");
        assert_eq!(leaves[..2], [pair("hyperv-switch", "Default Switch"), pair("route", "* vEthernet (Default Switch)")]);
        // Hyper-V's own firewall rules and vmms's listener (windows-bare's setup-hyperv audit):
        // the 14 Hyper-V rules by their whole names, the Host Network Service's two for the
        // Default Switch by theirs up to the network's id, port 2179 to vmms alone.
        let rules: Vec<&str> = leaves.iter().filter(|(k, _)| k == "firewall-rule").map(|(_, n)| n.as_str()).collect();
        assert_eq!(rules.len(), 16, "{rules:?}");
        assert!(rules.iter().all(|r| !r.contains('*') || r.starts_with("HNS Container Networking - ")), "{rules:?}");
        let ports: Vec<&str> = leaves.iter().filter(|(k, _)| k == "listening-port").map(|(_, n)| n.as_str()).collect();
        // And NetBIOS on the Default Switch's vEthernet adapter, which Windows opens on every adapter.
        assert_eq!(ports, [":::2179 (vmms, *)", "0.0.0.0:2179 (vmms, *)", "*:139 (System, *"]);
        assert_eq!(leaves.last(), Some(&pair(FOLDER.0, FOLDER.1)));
        assert_eq!(leaves.len(), 2 + 16 + 3 + 1, "{leaves:?}");
        assert_eq!(names("hyperv", "linux"), vec![pair(FOLDER.0, FOLDER.1)]);
    }

    #[test]
    fn the_virtualbox_path_leaves_the_host_only_adapter_and_its_route_on_each_system() {
        assert!(
            names("virtualbox", "linux").contains(&pair("vbox-hostonly", "vboxnet*"))
                && names("virtualbox", "linux").contains(&pair("route", "* dev vboxnet*"))
        );
        assert!(names("virtualbox", "macos").contains(&pair("vbox-hostonly", "vagrantnet-*")));
        assert!(names("virtualbox", "windows").contains(&pair("route", "* VirtualBox Host-Only*")));
        assert!(names("vmware-utility", "macos").contains(&pair("process", "*vagrant-vmware-utility*")));
    }

    #[test]
    fn the_launchers_package_leaves_its_registrations_and_the_other_items_only_the_entrys_folder() {
        let linux = names("launcher", "linux");
        assert!(linux.contains(&pair("os-registration", "/usr/share/applications/Cyber CTF.desktop")), "{linux:?}");
        let windows = names("launcher", "windows");
        assert!(windows.contains(&pair("os-registration", "HKCU\\Software\\Classes\\cyberctf")), "{windows:?}");
        assert!(windows.iter().any(|(_, n)| n.ends_with("Cyber CTF.lnk")) && windows.iter().any(|(_, n)| n.contains("Uninstall")), "{windows:?}");
        // Vagrant, a plugin, the Docker self-test and the example lab leave nothing the audit
        // sees beyond its allowlist (the boxes and images, the launcher's cache).
        for item in ["vagrant", "plugin:vagrant-libvirt", "selftest:docker", "lab:hello-stack", "engine:docker-engine"] {
            assert_eq!(names(item, "linux"), vec![pair(FOLDER.0, FOLDER.1)], "{item}");
        }
        // The entry's folder is named by its real path, as the audit names a temp folder.
        let on_windows = of("vagrant", "windows", Path::new(r"C:\Users\p\AppData\Local\Temp\cyberctf-guided-setup"));
        assert_eq!(on_windows, vec![Leaves { kind: "temp-folder".into(), name: r"C:\Users\p\AppData\Local\Temp\cyberctf-guided-setup".into() }]);
    }
}
