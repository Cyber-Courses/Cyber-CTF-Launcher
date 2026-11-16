use super::*;

#[test]
fn docker_install_adds_the_player_to_the_docker_group() {
    let s = docker_script(Some("player"));
    assert!(s.contains("usermod -aG docker player"), "{s}");
    // A name that isn't a plain login name never reaches the script.
    assert!(!docker_script(Some("a; rm -rf /")).contains("usermod"));
    assert!(!docker_script(None).contains("usermod"));
    assert!(valid_user("_svc-1.x") && !valid_user("Root") && !valid_user(""));
}

#[test]
fn libvirt_install_brings_what_the_vagrant_libvirt_plugin_compiles_against() {
    // Regression: libvirt alone, and `vagrant plugin install vagrant-libvirt` then failed
    // building its native extension (no libvirt headers, no compiler).
    let s = libvirt_script(None);
    for package in ["libvirt-daemon-system", "libvirt-dev", "build-essential", "pkg-config", "ruby-dev"] {
        assert!(s.split_whitespace().any(|w| w == package), "{package}: {s}");
    }
    assert!(!s.contains("usermod"));
    let s = libvirt_script(Some("ada"));
    assert!(s.contains("usermod -aG libvirt ada") && s.contains(&join_kvm_group("ada", "/dev/kvm")), "{s}");
    assert!(!libvirt_script(Some("a; rm -rf /")).contains("usermod"));
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
}

#[test]
fn qemu_install_adds_the_player_to_the_group_that_opens_dev_kvm() {
    // Regression: the QEMU install added nobody to the kvm group, so on Debian 13 (/dev/kvm
    // root:kvm 0660) every lab's `accel=kvm` was refused: "Could not access KVM kernel module:
    // Permission denied".
    let s = qemu_script(Some("ada"));
    let install = "apt-get update; apt-get install -y qemu-system qemu-utils genisoimage; \
                   if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi";
    assert_eq!(s, RootScript::new(format!("{install}; {}", join_kvm_group("ada", "/dev/kvm"))));
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    assert_eq!(qemu_script(None), RootScript::new(install));
    assert!(!qemu_script(Some("a; rm -rf /")).contains("usermod"));
}

#[test]
fn qemu_install_brings_the_mkisofs_vagrant_builds_a_vms_network_seed_with() {
    // Regression (an Ubuntu 24.04 attack box in a QEMU VM): the install was `qemu-system
    // qemu-utils`, and every VM-form lab failed at its first VM: "The executable 'mkisofs'
    // Vagrant is trying to run was not found in the PATH variable". `genisoimage` is the
    // package with /usr/bin/mkisofs on Debian and Ubuntu.
    for user in [None, Some("ada")] {
        let s = qemu_script(user);
        let install = s.split("; ").find(|c| c.contains("apt-get install")).unwrap();
        assert!(install.split_whitespace().any(|w| w == "genisoimage"), "{s}");
    }
}

/// Whether `script` exits non-zero when its package installs fail (`apt-get install` and
/// `dnf`, as when apt's lock is held or the network is down), the group steps after them
/// standing in for success.
#[cfg(unix)]
fn fails_when_its_package_manager_does(script: &str) -> bool {
    let bin = std::env::temp_dir().join(format!("cyberctf-pm-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("apt-get", r#"[ "$1" = update ] || exit 100"#),
        ("dnf", "exit 1"),
        ("apt-cache", "exit 0"),
        ("usermod", "exit 0"),
        ("getent", "exit 0"),
        ("systemctl", "exit 0"),
    ] {
        crate::testing::write_executable(bin.join(name), format!("#!/bin/sh\n{body}\n"));
    }
    let out = std::process::Command::new("sh").arg("-c").arg(script).env("PATH", format!("{}:/usr/bin:/bin", bin.display())).output().unwrap();
    std::fs::remove_dir_all(&bin).unwrap();
    !out.status.success()
}

#[cfg(unix)]
#[test]
fn a_failed_libvirt_or_qemu_install_fails_its_step() {
    // Regression: after a failed install the group step that followed decided the exit status,
    // and the install read as done. `set -e` doesn't stop at a failure inside an `&&` chain,
    // so the commands are separate statements.
    for (name, s) in [
        ("libvirt", libvirt_script(Some("ada"))),
        ("qemu", qemu_script(Some("ada"))),
        ("libvirt dnf", libvirt_dnf_script(Some("ada"))),
        ("qemu dnf", qemu_dnf_script(Some("ada"))),
    ] {
        assert!(s.starts_with(crate::platform::root_script::PRELUDE), "{name}: {s}");
        assert!(fails_when_its_package_manager_does(&s), "{name} reads as done after a failed install: {s}");
    }
}

/// What [`join_kvm_group`] does on a system whose `stat` says `owner` owns the device (empty:
/// no device) and whose groups are `groups`: the `usermod` it ran, if any, and what it said.
#[cfg(unix)]
fn joined(owner: &str, groups: &[&str]) -> (String, String) {
    let fakes = format!(
        "stat() {{ [ -n '{owner}' ] && echo '{owner}' || return 1; }}; getent() {{ for g in {}; do [ \"$g\" = \"$2\" ] && return 0; done; return 1; }}; usermod() {{ echo \"usermod $*\" >&2; }};",
        groups.join(" ")
    );
    let out = std::process::Command::new("sh").arg("-c").arg(format!("{fakes} {}", join_kvm_group("ada", "/dev/kvm"))).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    (String::from_utf8_lossy(&out.stderr).trim().to_owned(), String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

// Runs the step in `sh`, which Windows hasn't: the step is for Linux's installs only.
#[cfg(unix)]
#[test]
fn the_kvm_group_joined_is_the_devices_on_every_distribution() {
    // Debian, Ubuntu, Kali, openSUSE: /dev/kvm root:kvm 0660.
    let (ran, said) = joined("kvm", &["root", "kvm"]);
    assert_eq!(ran, "usermod -aG kvm ada");
    assert_eq!(said, "Added ada to the kvm group, which opens /dev/kvm: log out and back in (or restart) for it to apply.");
    // A distribution's own group for it.
    assert_eq!(joined("hypervisor", &["kvm", "hypervisor"]).0, "usermod -aG hypervisor ada");
    // Fedora and Arch (root:kvm 0666), or no device yet: the kvm group where there is one.
    assert_eq!(joined("root", &["kvm"]).0, "usermod -aG kvm ada");
    assert_eq!(joined("", &["kvm"]).0, "usermod -aG kvm ada");
    // No such group: nobody is added, and the install still succeeds.
    assert_eq!(joined("", &["root"]), (String::new(), String::new()));
}

#[test]
fn libvirt_install_brings_virtiofsd_for_the_shared_folder_where_the_release_has_it() {
    // Regression: libvirt alone, and an attack box with the shared folder failed to start on
    // libvirt ("Unable to find a satisfying virtiofsd").
    let s = libvirt_script(None);
    assert!(s.contains("if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi"), "{s}");
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
}

#[test]
fn qemu_install_brings_virtiofsd_for_the_shared_folder_where_the_release_has_it() {
    // Regression: QEMU alone, and an attack box with the shared folder failed to start on QEMU
    // on Linux: vagrant-qemu shares it through the virtiofsd it starts ("virtiofsd was not found
    // or is not executable").
    let s = qemu_script(None);
    assert!(s.contains("if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi"), "{s}");
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    assert!(qemu_dnf_script(None).split_whitespace().any(|w| w == "virtiofsd"), "dnf too");
}

#[test]
fn login_names_at_the_edges_of_what_goes_unquoted_into_the_script() {
    // 32 characters is the longest a login name can be; one more, a capital, a digit or a
    // dash first, a space or non-ASCII anywhere: not a name, so no `usermod` line.
    assert!(valid_user(&"a".repeat(32)) && !valid_user(&"a".repeat(33)));
    assert!(valid_user("a") && valid_user("_") && valid_user("a-") && valid_user("a.b"));
    for bad in ["1a", "-a", "A", "ab c", "caf\u{e9}", "a;b", "a$b", "a\nb"] {
        assert!(!valid_user(bad), "{bad:?}");
        assert!(!docker_script(Some(bad)).contains("usermod"), "{bad:?}");
    }
    // The group line names the player once, after the install, and the script stays one line.
    let s = docker_script(Some("ada"));
    assert_eq!(s.matches("usermod -aG docker ada").count(), 1);
    assert!(!s.contains('\n'));
}

#[cfg(target_os = "macos")]
#[test]
fn a_failed_cask_fetch_updates_homebrew_and_retries_once() {
    // A Homebrew whose fetch always fails (an old one crashing on the cask data).
    let dir = std::env::temp_dir().join(format!("cyberctf-brew-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let brew = dir.join("brew");
    let log = dir.join("calls");
    crate::testing::write_executable(&brew, format!("#!/bin/sh\necho \"$1\" >> '{}'\n[ \"$1\" = update ]\n", log.display()));

    let step = fetch_and_open(brew.to_str().unwrap(), "vagrant", "Vagrant");
    let out = std::process::Command::new(&step.program).args(&step.args).output().unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert!(!out.status.success());
    assert_eq!(calls.lines().collect::<Vec<_>>(), ["fetch", "update", "fetch"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Run 'brew update' in Terminal"));
}

/// Every vendor's apt repository, with the script that sets it up.
fn repo_scripts() -> [(&'static VendorRepo, RootScript); 7] {
    [
        (&HASHICORP, terraform_script()),
        (&HASHICORP, vagrant_script()),
        (&HASHICORP, vmware_utility_script()),
        (&ORACLE, virtualbox_script()),
        (&DOCKER, docker_script(None)),
        (&MICROSOFT, azurecli_script()),
        (&GOOGLE, gcloud_script()),
    ]
}

#[test]
fn repo_scripts_rerun_without_a_terminal_and_parse() {
    // gpg asked "overwrite?" on a second run, with no tty to answer it.
    for (_, s) in repo_scripts() {
        assert!(s.contains("gpg --batch --yes --dearmor"), "{s}");
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }
    assert!(virtualbox_script().contains("download.virtualbox.org/virtualbox/debian"));
    // VMware's utility from HashiCorp's repository, its key pinned like Terraform's; a release
    // the repository doesn't serve yet is sent to the utility's own page.
    let utility = vmware_utility_script();
    assert!(utility.contains(HASHICORP.fingerprint) && utility.contains("apt-get install -y --reinstall vagrant-vmware-utility"), "{utility}");
    assert!(utility.contains("developer.hashicorp.com/vagrant/install/vmware") && !utility.contains("terraform/install"), "{utility}");
}

#[test]
fn every_vendor_repository_is_set_up_the_same_way() {
    // Regression: five copies of one sequence had drifted (keyrings in /usr/share/keyrings or
    // /etc/apt/keyrings, an architecture on some sources and not others).
    for (repo, s) in repo_scripts() {
        let name = repo.name;
        assert!(s.contains(&format!("-o /etc/apt/keyrings/{name}.gpg")), "{s}");
        let source = format!("signed-by=/etc/apt/keyrings/{name}.gpg] $repo {} {}\" > /etc/apt/sources.list.d/{name}.list", repo.suite, repo.component);
        assert!(s.contains(&format!("deb [arch=$(dpkg --print-architecture) {source}")), "{s}");
        assert!(!s.contains("/usr/share/keyrings"), "{s}");
        assert_eq!(s.contains(&format!(r#"release_served "$repo" "{}""#, repo.suite)), repo.by_hand.is_some(), "{s}");
    }
}

/// Every install script that runs as root.
fn root_scripts() -> [RootScript; 8] {
    [
        awscli_script(),
        docker_script(Some("player")),
        terraform_script(),
        vagrant_script(),
        virtualbox_script(),
        azurecli_script(),
        gcloud_script(),
        vmware_utility_script(),
    ]
}

#[test]
fn every_root_script_that_downloads_installs_curl_first() {
    // Regression: Debian's minimal image has no curl; the Vagrant install failed, saying
    // HashiCorp had no packages for trixie, and Docker's repository check read as "not
    // served", so Docker came from the distribution instead.
    for s in root_scripts() {
        if let Some(first) = s.find("curl -") {
            let installed = s.find(NEED_CURL).unwrap_or_else(|| panic!("curl used, never installed: {s}"));
            assert!(installed < first, "curl used before it is installed: {s}");
        }
    }
}

/// Runs `script` with [`RELEASE_SERVED`] defined and a `curl` that answers HTTP status `http`:
/// its stdout and stderr.
#[cfg(unix)]
fn with_release_answer(http: &str, script: &str) -> (String, String) {
    let bin = std::env::temp_dir().join(format!("cyberctf-curl-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("curl");
    crate::testing::write_executable(&fake, format!("#!/bin/sh\nprintf '%s' '{http}'\n"));
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("set -e; {RELEASE_SERVED}; {script}"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    std::fs::remove_dir_all(&bin).unwrap();
    (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// Runs [`KERNEL_BUILD`] on a machine running kernel `running`, whose package sources have
/// `packages`: its `$build` and `$reboot`, else its stderr.
#[cfg(unix)]
fn kernel_build(running: &str, packages: &[&str]) -> Result<(String, String), String> {
    let bin = std::env::temp_dir().join(format!("cyberctf-kernel-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&bin).unwrap();
    let fake = |name: &str, body: String| {
        crate::testing::write_executable(bin.join(name), body);
    };
    fake("uname", format!("#!/bin/sh\necho '{running}'\n"));
    // `apt-cache policy <package>`: a candidate when the sources have it, none otherwise, its
    // label translated (as apt does under pkexec, which keeps `LANG`) unless `LC_ALL=C`.
    let known = packages.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(" ");
    fake(
        "apt-cache",
        format!(
            "#!/bin/sh\nlabel='Candidat :'; [ \"$LC_ALL\" = C ] && label='Candidate:'\nfor p in {known}; do [ \"$2\" = \"$p\" ] && {{ echo \"$p:\"; echo \"  $label 1.0-1\"; exit 0; }}; done\necho \"$2:\"; echo \"  $label (none)\"\n"
        ),
    );
    // Behind the prelude every root script starts with, under a French `LANG`.
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{}; {KERNEL_BUILD}; echo \"$build|$reboot\"", crate::platform::root_script::PRELUDE))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LANG", "fr_FR.UTF-8")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&bin).unwrap();
    let said = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let (build, reboot) = said.split_once('|').unwrap();
    Ok((build.to_owned(), reboot.to_owned()))
}

#[cfg(unix)]
#[test]
fn virtualbox_builds_its_driver_with_the_running_kernels_headers_or_waits_for_the_newer_kernel() {
    // Regression (a Debian 13 running 6.12.86 while its archive had 6.12.116): VirtualBox's
    // recommends brought linux-headers-amd64, the newest kernel's headers (and that kernel), never
    // the running one's, so its driver couldn't be built and VirtualBox read as installed with no
    // driver.
    let debian = "6.12.86+deb13-amd64";
    let headers = format!("linux-headers-{debian}");
    assert_eq!(
        kernel_build(debian, &[&headers, "linux-headers-amd64"]).unwrap(),
        ("gcc make perl kmod linux-headers-6.12.86+deb13-amd64 linux-headers-amd64".into(), String::new())
    );
    // Gone from the sources: the flavour's kernel and headers, and a restart.
    assert_eq!(kernel_build(debian, &["linux-headers-amd64"]).unwrap(), ("gcc make perl kmod linux-image-amd64 linux-headers-amd64".into(), "1".into()));
    // A flavour of its own (Debian's cloud kernel), Ubuntu's, Kali's.
    assert_eq!(
        kernel_build("6.12.86+deb13-cloud-amd64", &["linux-headers-cloud-amd64", "linux-headers-amd64"]).unwrap().0,
        "gcc make perl kmod linux-image-cloud-amd64 linux-headers-cloud-amd64"
    );
    assert_eq!(
        kernel_build("6.8.0-45-generic", &["linux-headers-6.8.0-45-generic", "linux-headers-generic"]).unwrap().0,
        "gcc make perl kmod linux-headers-6.8.0-45-generic linux-headers-generic"
    );
    assert_eq!(
        kernel_build("6.12.25-amd64", &["linux-headers-amd64"]).unwrap(),
        ("gcc make perl kmod linux-image-amd64 linux-headers-amd64".into(), "1".into())
    );
    // Neither: it stops, saying so, before anything is installed.
    let none = kernel_build(debian, &[]).unwrap_err();
    assert!(none.starts_with("VirtualBox builds its kernel driver with the running kernel's headers (linux-headers-6.12.86+deb13-amd64)"), "{none}");
}

#[test]
fn apt_cache_policy_is_read_in_english_whatever_the_language() {
    // Regression: under `LANG=fr_FR.UTF-8` apt prints `Candidat :`, so the `Candidate:` grep
    // never matched: the kernel headers read as missing and Ubuntu's own VirtualBox was missed.
    // The prelude, which every policy read comes after, sets the C locale.
    let s = virtualbox_script();
    let prelude = crate::platform::root_script::PRELUDE;
    assert!(prelude.ends_with("export LC_ALL=C") && s.starts_with(prelude), "{s}");
    assert_eq!(s.matches("apt-cache policy").count(), 3, "{s}");
}

#[test]
fn virtualbox_installs_without_its_recommends_with_what_builds_its_driver() {
    // Regression: its recommends brought a PDF viewer, Qt's translations and the newest kernel (325
    // MB, 154 packages on a bare Debian 13), and no driver.
    let s = virtualbox_script();
    assert!(s.contains("apt-get install -y --no-install-recommends virtualbox $build;"), "{s}");
    assert!(s.contains(r#"apt-get install -y --no-install-recommends "$pkg" $build;"#), "{s}");
    assert!(!s.contains("apt-get install -y virtualbox") && !s.contains(r#"apt-get install -y "$pkg""#), "{s}");
    assert!(s.find(KERNEL_BUILD).unwrap() < s.find("--no-install-recommends").unwrap(), "{s}");
    assert!(s.ends_with("restart the computer to start it, and VirtualBox's driver is built then.\""), "{s}");
    assert!(crate::platform::sh_parses(&s), "{s}");
}

#[cfg(unix)]
#[test]
fn a_repository_check_says_no_packages_only_when_the_release_has_none() {
    // Regression: any curl failure (curl missing, no network) read as "no packages for trixie".
    let s = apt_repo_script(&HASHICORP);
    let snippet = &s[s.find("; release_served \"$repo\"").unwrap() + 2..s.find("; command -v gpg").unwrap()];
    let check = |http: &str| with_release_answer(http, &format!("repo=https://apt.example; codename=trixie; {snippet}; echo went-on"));
    assert_eq!(check("200").0.trim(), "went-on");
    let missing = check("404").1;
    assert!(missing.starts_with("HashiCorp has no packages for trixie yet"), "{missing}");
    let unreachable = check("000").1;
    assert!(unreachable.starts_with("Couldn't reach https://apt.example (HTTP 000)") && !unreachable.contains("no packages"), "{unreachable}");
}

#[cfg(unix)]
#[test]
fn a_debian_derivative_the_vendor_doesnt_serve_takes_its_debian_bases_packages() {
    // Regression: Kali (kali-rolling) stopped at "HashiCorp has no packages for kali-rolling
    // yet"; HashiCorp serves trixie and bookworm.
    let s = apt_repo_script(&HASHICORP);
    let snippet = &s[s.find("; release_served \"$repo\"").unwrap() + 2..s.find("; command -v gpg").unwrap()];
    let bin = std::env::temp_dir().join(format!("cyberctf-curl-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("curl");
    crate::testing::write_executable(&fake, "#!/bin/sh\ncase \"$*\" in */dists/trixie/*) printf 200 ;; *) printf 404 ;; esac\n");
    let run = |vars: &str| {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("set -e; {RELEASE_SERVED}; repo=https://apt.example; {vars}; {snippet}; echo \"went-on $codename\""))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (out, _) = run("codename=kali-rolling; debian_base='trixie bookworm'");
    assert!(out.contains("has no packages for kali-rolling: installing its trixie ones") && out.trim_end().ends_with("went-on trixie"), "{out}");
    let (out, err) = run("codename=kali-rolling; debian_base=''");
    assert!(!out.contains("went-on") && err.starts_with("HashiCorp has no packages for kali-rolling yet"), "{out} / {err}");
    let (out, err) = run("codename=noble; debian_base='trixie bookworm'; unset debian_base");
    assert!(!out.contains("went-on") && err.starts_with("HashiCorp has no packages for noble yet"), "{out} / {err}");
    std::fs::remove_dir_all(&bin).unwrap();
}

#[cfg(unix)]
#[test]
fn dockers_repository_check_gives_the_distributions_docker_only_on_a_404() {
    // Regression: any failure of Docker's probe (DNS, a proxy, a timeout) installed docker.io,
    // saying Docker's repository had no packages for the release.
    let dir = std::env::temp_dir().join(format!("cyberctf-docker-probe-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let release = dir.join("os-release");
    std::fs::write(&release, "ID=debian\nVERSION_CODENAME=trixie\n").unwrap();
    let probe = DOCKER_DIST.replace("/etc/os-release", &release.display().to_string()).replace("command -v apt-get >/dev/null", "true");
    let check = |http: &str| with_release_answer(http, &format!("{probe}; echo \"$dist/$codename/$own\""));
    assert_eq!(check("200").0.trim(), "debian/trixie/");
    assert_eq!(check("404").0.trim(), "debian/trixie/1");
    for unreachable in ["000", "407", "503"] {
        let (out, err) = check(unreachable);
        assert!(out.is_empty(), "went on after HTTP {unreachable}: {out}");
        assert!(err.starts_with(&format!("Couldn't reach https://download.docker.com/linux/debian (HTTP {unreachable})")), "{err}");
    }
    // The script asks once, and defines the probe before it asks.
    let s = docker_script(None);
    assert_eq!(s.matches(NEED_CURL).count(), 1, "{s}");
    assert_eq!(s.matches("release_served()").count(), 1, "{s}");
    assert!(s.find("release_served()").unwrap() < s.find("! release_served").unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn vagrant_names_its_own_install_page() {
    assert!(vagrant_script().contains("https://developer.hashicorp.com/vagrant/install"), "not Terraform's page");
}

#[test]
fn root_scripts_never_use_fixed_temp_paths_or_pipe_downloads_into_a_shell() {
    // Under pkexec the AWS CLI was unzipped into /tmp/aws and /tmp/aws/install run as root:
    // any local user could plant that file first.
    // Every download fails on an HTTP error (`-f`); the one curl that doesn't is the
    // repository check, which reads the status code itself (RELEASE_SERVED).
    const STATUS_PROBE: &str = "s -o /dev/null -w '%{http_code}'";
    assert!(RELEASE_SERVED.contains(&format!("curl -{STATUS_PROBE}")));
    for s in root_scripts() {
        assert!(!s.contains("/tmp"), "fixed temp path: {s}");
        assert!(!s.contains("| sh") && !s.contains("| bash"), "download piped into a shell: {s}");
        for call in s.split("curl -").skip(1) {
            assert!(call.starts_with('f') || call.starts_with(STATUS_PROBE), "a failed download must fail: curl -{call}");
        }
        assert!(s.starts_with("set -e"), "{s}");
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }
    for s in root_scripts() {
        assert!(s.contains(r#"d="$(mktemp -d)""#) && s.contains("trap 'rm -rf \"$d\"' EXIT"), "{s}");
    }
    assert!(awscli_script().contains(r#""$d/aws/install" --update"#) && awscli_script().contains("unzip -tq"));
}

/// Whether `program` runs here (the gpg checks run for real where gpg is installed).
#[cfg(unix)]
fn have(program: &str) -> bool {
    std::process::Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Runs `script` with `sh`, in a fresh directory `$d` holding `files`: its exit success and
/// its stderr.
#[cfg(unix)]
fn run_in_workdir(script: &str, files: &[(&str, &[u8])]) -> (bool, String) {
    let d = std::env::temp_dir().join(format!("cyberctf-script-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&d).unwrap();
    for (name, body) in files {
        std::fs::write(d.join(name), body).unwrap();
    }
    let out = std::process::Command::new("sh").arg("-c").arg(format!("set -e; d='{}'; {script}", d.display())).output().unwrap();
    let _ = std::fs::remove_dir_all(&d);
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn vendor_keys_are_pinned_and_checked_before_apt_trusts_them() {
    let vendors = repo_scripts().map(|(repo, _)| repo.fingerprint);
    for fpr in vendors.into_iter().chain([AWS_CLI_KEY_FINGERPRINT]) {
        assert!(fpr.len() == 40 && fpr.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)), "{fpr}");
    }
    // The key is checked, then installed for apt: never fetched straight into the keyring.
    let check = |s: &str, fpr: &str, install: &str| {
        let checked = s.find(&format!(r#"[ "$k" = "{fpr}" ]"#)).unwrap_or_else(|| panic!("no fingerprint check: {s}"));
        let installed = s.find(install).unwrap_or_else(|| panic!("no install of the key: {s}"));
        assert!(s.find(r#"-o "$d/key""#).is_some_and(|fetched| fetched < checked) && checked < installed, "{s}");
    };
    for (repo, s) in repo_scripts() {
        check(&s, repo.fingerprint, &format!("-o /etc/apt/keyrings/{}.gpg", repo.name));
        assert!(!s.contains("gpg | gpg"), "{s}");
        assert!(s.contains(r#"--homedir "$d/gnupg""#) && !s.contains("--keyserver"), "{s}");
    }
    assert!(gcloud_script().contains(r#"repo="https://packages.cloud.google.com/apt"; "#));
}

#[test]
fn the_aws_cli_bundle_must_be_signed_by_the_embedded_key() {
    let s = awscli_script();
    assert!(AWS_CLI_KEY.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----") && AWS_CLI_KEY.trim_end().ends_with("-----END PGP PUBLIC KEY BLOCK-----"));
    assert!(s.contains(AWS_CLI_KEY.trim_end()), "the key is in the script");
    assert!(s.contains(r#"curl -fsSL "$url.sig" -o "$d/awscliv2.zip.sig""#), "{s}");
    // Verified before it is unpacked, let alone run.
    let verified = s.find(&format!("VALIDSIG {AWS_CLI_KEY_FINGERPRINT} ")).unwrap();
    assert!(verified < s.find("unzip -tq").unwrap() && verified < s.find(r#""$d/aws/install""#).unwrap(), "{s}");
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
}

#[test]
fn no_vendor_install_script_runs_as_root() {
    // Regression: Docker (on distros without its apt repository), Azure CLI and gcloud ran
    // the vendor's install script (get.docker.com, aka.ms/InstallAzureCLIDeb,
    // sdk.cloud.google.com), which fetches its own key: no fingerprint held it.
    for s in root_scripts() {
        for script in ["get.docker.com", "aka.ms/InstallAzureCLIDeb", "sdk.cloud.google.com", "install.sh"] {
            assert!(!s.contains(script), "{script} in {s}");
        }
    }
}

#[cfg(unix)]
#[test]
fn docker_without_its_apt_repository_stops_and_says_where_to_get_it() {
    // Only the branch past the distro check: `dist` empty, nothing else run. Regression: the
    // slice began at the distro detection's own `[ -n "$dist" ]`, a syntax error that never ran
    // the refusal; bash echoes the failing line (URL included), so it passed on macOS only.
    let s = docker_script(None);
    let refuse = &s[s.find("[ -n \"$dist\" ] ||").unwrap()..s.find("; if [ -n \"$own\" ]").unwrap()];
    let out = std::process::Command::new("sh").arg("-c").arg(format!("dist=''; {refuse}; echo installed")).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "the refusal's own exit, not a syntax error: {err}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("installed"));
    assert!(err.starts_with("Cyber CTF installs Docker on Debian") && err.contains("https://docs.docker.com/engine/install/"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_key_with_another_fingerprint_stops_the_install() {
    if !have("gpg") {
        eprintln!("no gpg here: the key check isn't run");
        return;
    }
    let pinned = format!(r#"mkdir -p -m 700 "$d/gnupg"; {}; echo checked >&2"#, check_keys(&[AWS_CLI_KEY_FINGERPRINT], "AWS"));
    let (ok, err) = run_in_workdir(&pinned, &[("key", AWS_CLI_KEY.as_bytes())]);
    assert!(ok && err.contains("checked"), "{err}");
    let other = format!(r#"mkdir -p -m 700 "$d/gnupg"; {}; echo checked >&2"#, check_keys(&[DOCKER.fingerprint], "Docker"));
    let (ok, err) = run_in_workdir(&other, &[("key", AWS_CLI_KEY.as_bytes())]);
    assert!(!ok && !err.contains("checked"), "{err}");
    assert!(
        err.contains(
            "Docker's signing key isn't the expected one (fingerprint 9DC858229FC7DD38854AE2D88D81803C0EBFCD88, got: FB5DB77FD5C118B80511ADA8A6310ACC4672475C)"
        ),
        "{err}"
    );
    let (ok, err) = run_in_workdir(&other, &[("key", b"<html>captive portal</html>")]);
    assert!(!ok && err.contains("got: )"), "{err}");
}

#[cfg(unix)]
#[test]
fn an_aws_cli_bundle_with_a_bad_signature_is_not_installed() {
    if !have("gpg") || !have("gpgv") {
        eprintln!("no gpg here: the signature check isn't run");
        return;
    }
    let verify = format!("{}; echo verified >&2", aws_verify());
    let (ok, err) = run_in_workdir(&verify, &[("awscliv2.zip", b"PK not really"), ("awscliv2.zip.sig", b"not a signature")]);
    assert!(!ok && !err.contains("verified"), "{err}");
    assert!(err.contains("The AWS CLI download isn't signed by AWS's key (FB5DB77FD5C118B80511ADA8A6310ACC4672475C)"), "{err}");
}

#[cfg(unix)]
#[test]
fn docker_picks_its_apt_repository_by_distro_or_the_distributions_own_docker() {
    let dir = std::env::temp_dir().join(format!("cyberctf-os-release-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let release = dir.join("os-release");
    assert!(DOCKER_DIST.contains(". /etc/os-release;") && DOCKER_DIST.contains("command -v apt-get >/dev/null"));
    let probe = DOCKER_DIST.replace("/etc/os-release", &release.display().to_string()).replace("command -v apt-get >/dev/null", "true");
    // Docker's repository as it is: the releases it serves, per distribution.
    let serves = r#"release_served() { case "$1/$2" in */debian/bookworm|*/debian/trixie|*/ubuntu/jammy|*/ubuntu/noble|*/raspbian/bookworm) return 0;; esac; return 1; }"#;
    let pick = |os: &str| {
        std::fs::write(&release, os).unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(format!("set -e; {serves}; {probe}; echo \"$dist/$codename/$own\"")).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    assert_eq!(pick("ID=ubuntu\nID_LIKE=debian\nVERSION_CODENAME=noble\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble/");
    assert_eq!(pick("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_CODENAME=wilma\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble/");
    assert_eq!(pick("ID=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie/");
    assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=bookworm\n"), "raspbian/bookworm/");
    assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie/");
    // A Debian derivative on the Debian release it names (LMDE): Docker's Debian repository.
    assert_eq!(pick("ID=linuxmint\nID_LIKE=debian\nVERSION_CODENAME=faye\nDEBIAN_CODENAME=bookworm\n"), "debian/bookworm/");
    // Regression: Kali (and Devuan) had no repository at all and the install stopped. Docker
    // serves neither release: the distribution's own Docker.
    assert_eq!(pick("ID=kali\nID_LIKE=debian\nVERSION_CODENAME=kali-rolling\nVERSION_ID=\"2025.4\"\n"), "debian/kali-rolling/1");
    assert_eq!(pick("ID=devuan\nID_LIKE=debian\nVERSION_CODENAME=excalibur\n"), "debian/excalibur/1");
    // A release newer than Docker serves yet: the distribution's own too.
    assert_eq!(pick("ID=debian\nVERSION_CODENAME=forky\n"), "debian/forky/1");
    assert_eq!(pick("ID=ubuntu\nID_LIKE=debian\nVERSION_CODENAME=resolute\nUBUNTU_CODENAME=resolute\n"), "ubuntu/resolute/1");
    // Neither Debian nor Ubuntu: nothing from here.
    assert_eq!(pick("ID=fedora\nVERSION_ID=43\n"), "//");
    assert_eq!(pick("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n"), "//");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn where_dockers_repository_has_no_release_the_distributions_own_docker_and_compose_go_in() {
    let s = docker_script(Some("ada"));
    let own = &s[s.find("if [ -n \"$own\" ]; then").unwrap()..s.find("; else repo=").unwrap()];
    assert!(own.contains("apt-get install -y docker.io") && own.contains("docker-compose-v2") && own.contains("apt-get install -y docker-compose"), "{own}");
    assert!(!own.contains("docker-ce") && !own.contains("download.docker.com/linux/$dist"), "{own}");
    // Either way the player joins the docker group, and the script parses.
    assert!(s.ends_with("echo 'Added ada to the docker group: log out and back in (or restart) for it to apply.'"), "{s}");
    assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
}
