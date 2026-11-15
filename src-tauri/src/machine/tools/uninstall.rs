//! Removing what Cyber CTF installed. Each successful assisted install is recorded, and Cleanup
//! offers to remove those tools only (a Docker or Vagrant the player had before is never
//! touched). Removal goes through the same trusted flows as install: the package manager,
//! the vendor's own uninstaller, or the OS admin prompt.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::install::Dependency;
use super::steps::{Step, run_steps, step};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::host::{Host, LinuxPackages, Os, Packages};

/// A tool Cyber CTF installed, as Settings lists it: the record, and its name here.
#[derive(Serialize, Clone, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "InstalledTool")]
pub struct Installed {
    /// The tool.
    pub dependency: Dependency,
    /// Its name on this OS ([`Dependency::name`]).
    pub name: &'static str,
    /// When Cyber CTF installed it, in seconds since the Unix epoch.
    pub at: u64,
}

/// A tool Cyber CTF installed, on record so Cleanup removes only what it installed.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
    /// The tool.
    pub dependency: Dependency,
    /// When Cyber CTF installed it, in seconds since the Unix epoch.
    pub at: u64,
    /// The install only opened the vendor's own installer (macOS casks) or the tool isn't found
    /// yet: it counts as installed by Cyber CTF once a later check finds it. Never shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

/// How long an opened installer may take to be finished before the record is dropped: a tool that
/// shows up later than that was most likely installed some other way.
const PENDING_FOR_SECS: u64 = 24 * 3600;

/// What an install leaves on record, from whether the tool was there before and after it.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Outcome {
    /// Already there before: someone else installed it, so Cleanup must never remove it.
    NotOurs,
    /// Missing before, found now: Cyber CTF installed it.
    Installed,
    /// Missing before and still not found (an installer left open, PATH not refreshed yet):
    /// confirmed by a later check.
    Pending,
}

/// What an install leaves on record, from whether the tool was there before it (`present_before`)
/// and after it (`present_after`).
pub fn outcome(present_before: bool, present_after: bool) -> Outcome {
    match (present_before, present_after) {
        (true, _) => Outcome::NotOurs,
        (false, true) => Outcome::Installed,
        (false, false) => Outcome::Pending,
    }
}

/// Whether `dep` is installed on this machine; None when that can't be told (WSL).
pub async fn present(dep: Dependency) -> Option<bool> {
    // A service, not a program on PATH: what the machine check reads.
    if dep == Dependency::VmwareUtility {
        return Some(crate::runtime::providers::vmware_utility().await != crate::runtime::providers::UtilityState::Missing);
    }
    let row = dep.row();
    let mac_app = row.mac_app.filter(|_| Os::HERE == Os::Macos);
    if mac_app.is_some_and(|a| std::path::Path::new(a).exists()) {
        return Some(true);
    }
    match row.probe {
        Some((program, args)) => Some(crate::platform::exec::run_read(program, args, None).await.is_ok()),
        None if mac_app.is_some() => Some(false),
        None => None,
    }
}

/// Records the outcome of an install of `dependency`.
pub fn record_outcome(dirs: &AppDirs, dependency: Dependency, outcome: Outcome) {
    if outcome != Outcome::NotOurs {
        update(dirs, |tools| remember(tools, dependency, outcome));
    }
}

/// Promotes the pending records whose tool is now found, and drops those pending for too long.
fn settle(tools: &mut Vec<InstalledTool>, found: &[Dependency], now: u64) {
    for t in tools.iter_mut().filter(|t| t.pending && found.contains(&t.dependency)) {
        t.pending = false;
    }
    tools.retain(|t| !t.pending || now.saturating_sub(t.at) < PENDING_FOR_SECS);
}

/// Re-checks the installs that only opened an installer: run when the tools are listed.
async fn confirm_pending(dirs: &AppDirs) {
    let waiting: Vec<Dependency> = load_or_logged(dirs).into_iter().filter(|t| t.pending).map(|t| t.dependency).collect();
    if waiting.is_empty() {
        return;
    }
    let mut found = Vec::new();
    for dep in waiting {
        if present(dep).await == Some(true) {
            found.push(dep);
        }
    }
    let now = crate::platform::clock::unix_secs();
    update(dirs, |tools| {
        settle(tools, &found, now);
    });
}

/// The records; none without a data folder.
fn load(dirs: &AppDirs) -> Result<Vec<InstalledTool>> {
    dirs.data().map_or(Ok(Vec::new()), load_at)
}

/// [`load`], a record that can't be read logged and read as none (for listing them).
fn load_or_logged(dirs: &AppDirs) -> Vec<InstalledTool> {
    load(dirs).unwrap_or_else(|e| {
        log::warn!("the record of the tools Cyber CTF installed can't be read: {e}");
        Vec::new()
    })
}

/// The records in data folder `data`'s store: none when nothing was recorded; an error when they
/// can't be read, which must not read as "nothing installed".
fn load_at(data: &Path) -> Result<Vec<InstalledTool>> {
    crate::store::open_in(data)?.read(crate::store::tools::all)
}

/// Changes the records with `change`. One that can't be changed (unreadable, damaged, or it can't
/// be written) is left as it is and logged: Cleanup then doesn't offer to remove that tool.
fn update(dirs: &AppDirs, change: impl FnOnce(&mut Vec<InstalledTool>)) {
    if let Ok(data) = dirs.data()
        && let Err(e) = update_at(data, change)
    {
        log::warn!("the record of the tools Cyber CTF installed couldn't be updated: {e}");
    }
}

/// Changes the records in data folder `data`'s store with `change`, in one transaction: installs
/// and removals in different windows (machine setup, server setup, Settings) can end at the same
/// moment, and each keeps the other's change.
fn update_at(data: &Path, change: impl FnOnce(&mut Vec<InstalledTool>)) -> Result<()> {
    crate::store::open_in(data)?.write(|tx| {
        let mut tools = crate::store::tools::all(tx)?;
        change(&mut tools);
        crate::store::tools::replace(tx, &tools)
    })
}

/// `tools` with `dependency` recorded once as the install's `outcome` says: installed now, or
/// waiting for its installer (one not ours is not recorded). A tool already confirmed as ours
/// stays confirmed.
fn remember(tools: &mut Vec<InstalledTool>, dependency: Dependency, outcome: Outcome) {
    let waiting = match outcome {
        Outcome::NotOurs => return,
        Outcome::Installed => false,
        Outcome::Pending => true,
    };
    let pending = waiting && !tools.iter().any(|t| t.dependency == dependency && !t.pending);
    tools.retain(|t| t.dependency != dependency);
    tools.push(InstalledTool { dependency, at: crate::platform::clock::unix_secs(), pending });
}

/// The confirmed records only.
fn confirmed(tools: Vec<InstalledTool>) -> Vec<InstalledTool> {
    tools.into_iter().filter(|t| !t.pending).collect()
}

/// The tools Cyber CTF installed on this machine (and hasn't removed since). Installs that only
/// opened the vendor's installer count once the tool is found.
pub async fn installed_tools(dirs: AppDirs) -> Vec<Installed> {
    confirm_pending(&dirs).await;
    confirmed(load_or_logged(&dirs)).into_iter().map(|t| Installed { dependency: t.dependency, name: t.dependency.name(), at: t.at }).collect()
}

/// Runs a shell script with the OS admin prompt on macOS (the same dialog an installer shows).
fn mac_admin(script: &str, note: &str) -> Step {
    let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
    Step {
        program: "osascript".into(),
        args: vec!["-e".into(), format!("do shell script \"{escaped}\" with administrator privileges")],
        note: Some(note.into()),
        private: None,
    }
}

/// VirtualBox's own uninstaller, run as admin, only ever from the installer disk image Homebrew
/// downloaded (`dmg`, attached here, then detached). Never from whatever volume happens to be
/// mounted as `/Volumes/VirtualBox*`: anyone can mount a disk image with that name and a
/// `VirtualBox_Uninstall.tool` of their own, which would then run as root. The image is found
/// before elevating: as root, `~` isn't the player's home.
fn virtualbox_uninstall(dmg: &str) -> String {
    format!(
        "m=$(hdiutil attach -nobrowse -noautoopen '{dmg}' | awk -F'\\t' '/\\/Volumes\\//{{print $NF; exit}}'); [ -n \"$m\" ] || exit 3; \
         \"$m/VirtualBox_Uninstall.tool\" --unattended; s=$?; hdiutil detach -quiet \"$m\"; exit $s"
    )
}

/// The VirtualBox installer image, from `brew --cache --cask virtualbox`'s output: a `.dmg` path
/// the admin script can quote.
fn virtualbox_dmg_from(out: &str) -> Result<String> {
    let path = out.trim();
    // Homebrew names it `….dmg`, in lower case: anything else isn't its installer image.
    let is_dmg = std::path::Path::new(path).extension().is_some_and(|e| e == "dmg");
    if path.is_empty() || !is_dmg || path.contains(['\'', '"', '\n']) {
        return Err(Error::Invalid("Homebrew didn't say where VirtualBox's installer image is, so its uninstaller can't be run.".into()));
    }
    Ok(path.to_string())
}

/// The VirtualBox installer image Homebrew has (fetched again first: Homebrew drops the download
/// once the cask moves on, which it does often), as the player, before the admin step.
async fn virtualbox_dmg(brew: &'static str, on_line: &mut impl FnMut(String)) -> Result<String> {
    run_steps(&[step(brew, &["fetch", "--cask", "virtualbox"])], on_line).await?;
    let out = crate::platform::exec::run_cli(brew, &["--cache", "--cask", "virtualbox"]).await?;
    let dmg = virtualbox_dmg_from(&out)?;
    if !std::path::Path::new(&dmg).is_file() {
        return Err(Error::Invalid(format!(
            "VirtualBox's installer image isn't there ({dmg}), so its uninstaller can't be run. Run `brew fetch --cask virtualbox`, then try again."
        )));
    }
    Ok(dmg)
}

/// How to remove `dep` on macOS: Homebrew's own packages, or the vendor's uninstaller. Only
/// Homebrew's packages (and VirtualBox's installer image, which Homebrew keeps) need Homebrew: a
/// tool installed from its vendor's package is removed without it.
fn macos_plan(dep: Dependency, packages: Option<Packages>, virtualbox_dmg: Option<&str>) -> Result<Vec<Step>> {
    let brew = || match packages {
        Some(Packages::Homebrew(brew)) => Ok(brew),
        _ => Err(Error::Invalid("Homebrew is needed to remove this tool.".into())),
    };
    Ok(match dep {
        Dependency::Terraform => vec![step(brew()?, &["uninstall", "hashicorp/tap/terraform"])],
        Dependency::Qemu => vec![step(brew()?, &["uninstall", "qemu"])],
        Dependency::Awscli => vec![step(brew()?, &["uninstall", "awscli"])],
        Dependency::Gcloud => vec![step(brew()?, &["uninstall", "--cask", "google-cloud-sdk"])],
        Dependency::Azurecli => vec![step(brew()?, &["uninstall", "--cask", "azure-cli-preview"])],
        // Installed from the vendors' own packages: removed the way their uninstallers do.
        Dependency::Vagrant => vec![mac_admin(
            "rm -rf /opt/vagrant /usr/local/bin/vagrant && pkgutil --forget com.vagrant.vagrant || true",
            "Removing Vagrant (macOS asks for your password)…",
        )],
        Dependency::Virtualbox => {
            brew()?;
            let dmg = virtualbox_dmg.ok_or_else(|| Error::Invalid("VirtualBox's installer image is needed to run its uninstaller.".into()))?;
            vec![mac_admin(&virtualbox_uninstall(dmg), "Running VirtualBox's own uninstaller (macOS asks for your password)…")]
        }
        Dependency::Docker => vec![Step {
            program: "/Applications/Docker.app/Contents/MacOS/uninstall".into(),
            args: vec![],
            note: Some("Running Docker Desktop's own uninstaller…".into()),
            private: None,
        }],
        Dependency::Utm => vec![step("osascript", &["-e", "tell application \"Finder\" to delete POSIX file \"/Applications/UTM.app\""])],
        Dependency::VmwareUtility => {
            vec![mac_admin(super::vmware_utility::MAC_UNINSTALL, "Removing the Vagrant VMware Utility (macOS asks for your password)…")]
        }
        Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => return Err(super::install::not_here(dep, Os::Macos)),
    })
}

/// How to remove `dep` on Windows: its winget package, VMware's utility by its product code, and
/// the Windows features by the player in Windows Features.
fn windows_plan(dep: Dependency) -> Result<Vec<Step>> {
    if let Some(id) = dep.row().winget {
        return Ok(vec![super::install::winget("uninstall", id, &[])]);
    }
    match dep {
        Dependency::VmwareUtility => Ok(vec![super::vmware_utility::windows_uninstall()]),
        Dependency::Wsl => Err(Error::Invalid("Turn WSL off in Windows Features if you no longer need it.".into())),
        Dependency::Hyperv => Err(Error::Invalid("Turn Hyper-V off in Windows Features if you no longer need it.".into())),
        Dependency::Utm | Dependency::Libvirt | Dependency::Qemu => Err(super::install::not_here(dep, Os::Windows)),
        Dependency::Docker
        | Dependency::Vagrant
        | Dependency::Terraform
        | Dependency::Virtualbox
        | Dependency::Awscli
        | Dependency::Azurecli
        | Dependency::Gcloud => Err(Error::Invalid(format!("{} has no winget package to remove.", dep.name_on(Os::Windows)))),
    }
}

/// How to remove `dep` on Linux: its packages through the package manager that installed them
/// (apt or dnf, as root through pkexec), or its folder. Regression: a tool the setup had
/// installed through dnf on Fedora was removed with `apt-get remove`, which pkexec ran to
/// `apt-get: not found`, the record kept.
fn linux_plan(dep: Dependency, packages: Option<Packages>) -> Result<Vec<Step>> {
    let root = |script: &str| super::install::root(crate::platform::root_script::RootScript::new(script));
    // AWS CLI is a folder on every distribution.
    if dep == Dependency::Awscli {
        return Ok(vec![root("rm -rf /usr/local/aws-cli /usr/local/bin/aws /usr/local/bin/aws_completer")?]);
    }
    let Some(packages) = packages.and_then(Packages::linux) else {
        return Err(super::install::without_packages(dep));
    };
    let script = match (dep, packages) {
        (Dependency::Utm | Dependency::Wsl | Dependency::Hyperv, _) => return Err(super::install::not_here(dep, Os::Linux)),
        (Dependency::Awscli, _) => "rm -rf /usr/local/aws-cli /usr/local/bin/aws /usr/local/bin/aws_completer",
        // Fedora: what the dnf install brought from the vendors' repositories and RPM Fusion; the
        // compiler, Ruby's headers and the shared tools (genisoimage) stay.
        (Dependency::Docker, LinuxPackages::Dnf) => "dnf remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
        (Dependency::Docker, LinuxPackages::Apt) => "apt-get remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
        (Dependency::Vagrant, LinuxPackages::Dnf) => "dnf remove -y vagrant",
        (Dependency::Vagrant, LinuxPackages::Apt) => "apt-get remove -y vagrant",
        (Dependency::Terraform, LinuxPackages::Dnf) => "dnf remove -y terraform",
        (Dependency::Terraform, LinuxPackages::Apt) => "apt-get remove -y terraform",
        (Dependency::Virtualbox, LinuxPackages::Dnf) => "dnf remove -y VirtualBox akmod-VirtualBox 'kmod-VirtualBox*'",
        (Dependency::Virtualbox, LinuxPackages::Apt) => "apt-get remove -y 'virtualbox*'",
        // The install's metapackages (qemu-system, libvirt-daemon-system) leave the packages
        // they pulled in behind, with the binaries the Machine page looks for
        // (qemu-system-x86_64, virsh): those go too, or the tool still reads as installed.
        (Dependency::Qemu, LinuxPackages::Dnf) => "dnf remove -y qemu-kvm qemu-img",
        (Dependency::Qemu, LinuxPackages::Apt) => "apt-get remove -y 'qemu-system*' qemu-utils",
        (Dependency::Libvirt, LinuxPackages::Dnf) => "dnf remove -y libvirt libvirt-daemon-kvm virt-install virt-manager libvirt-devel",
        (Dependency::Libvirt, LinuxPackages::Apt) => "apt-get remove -y 'libvirt-daemon*' libvirt-clients virt-manager",
        (Dependency::Azurecli, LinuxPackages::Apt) => "apt-get remove -y azure-cli",
        (Dependency::Gcloud, LinuxPackages::Apt) => "apt-get remove -y google-cloud-cli",
        (Dependency::VmwareUtility, LinuxPackages::Apt) => "apt-get remove -y vagrant-vmware-utility",
        // The cloud CLIs and VMware's utility install through apt only: nothing of ours to remove.
        (Dependency::Azurecli | Dependency::Gcloud | Dependency::VmwareUtility, LinuxPackages::Dnf) => return Err(super::install::apt_only(dep)),
    };
    let mut steps = vec![root(script)?];
    // Google's apt package, and the per-user SDK earlier versions installed in its place.
    if dep == Dependency::Gcloud {
        steps.push(step("sh", &["-c", "rm -rf \"$HOME/google-cloud-sdk\""]));
    }
    Ok(steps)
}

/// How to remove `dep` on `host`. On macOS, VirtualBox's step needs its installer image
/// (`virtualbox_dmg`).
pub(super) fn plan_on(dep: Dependency, host: Host, virtualbox_dmg: Option<&str>) -> Result<Vec<Step>> {
    if !dep.runs_on(host.os) {
        return Err(super::install::not_here(dep, host.os));
    }
    match host.os {
        Os::Macos => macos_plan(dep, host.packages, virtualbox_dmg),
        Os::Windows => windows_plan(dep),
        Os::Linux => linux_plan(dep, host.packages),
    }
}

/// Removes a tool Cyber CTF installed, streaming the output, then forgets it.
pub async fn uninstall_dependency(dirs: AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    super::install::installs_allowed(&mut on_line)?;
    let recorded = load(&dirs).map_err(|e| Error::Invalid(format!("the record of the tools Cyber CTF installed can't be read: {e}")))?;
    if !confirmed(recorded).iter().any(|t| t.dependency == dependency) {
        return Err(Error::Invalid("Cyber CTF didn't install this tool, so it won't remove it.".into()));
    }
    let host = Host::here();
    let dmg = match (host.os, host.packages, dependency) {
        (Os::Macos, Some(Packages::Homebrew(brew)), Dependency::Virtualbox) => Some(virtualbox_dmg(brew, &mut on_line).await?),
        _ => None,
    };
    run_steps(&plan_on(dependency, host, dmg.as_deref())?, &mut on_line).await?;
    update(&dirs, |tools| tools.retain(|t| t.dependency != dependency));
    on_line("Removed.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BREW: &str = "/opt/homebrew/bin/brew";
    const MAC: Host = Host { os: Os::Macos, arch: crate::platform::host::Arch::Aarch64, packages: Some(Packages::Homebrew(BREW)) };
    const WINDOWS: Host = Host { os: Os::Windows, arch: crate::platform::host::Arch::X86_64, packages: Some(Packages::Winget) };
    const APT: Host = Host { os: Os::Linux, arch: crate::platform::host::Arch::X86_64, packages: Some(Packages::Linux(LinuxPackages::Apt)) };
    const DNF: Host = Host { packages: Some(Packages::Linux(LinuxPackages::Dnf)), ..APT };

    #[test]
    fn a_removal_runs_as_root_as_an_install_does() {
        // Regression: removals built their own pkexec line, without the proxy installs hand it.
        let steps = plan_on(Dependency::Vagrant, APT, None).unwrap();
        assert_eq!(steps[0].program, "pkexec");
        assert_eq!(steps[0].args.first().map(String::as_str), Some("env"));
        let script = crate::platform::root_script::RootScript::new("apt-get remove -y vagrant");
        assert_eq!(steps[0].args.last().map(String::as_str), Some(&*script));
    }

    #[tokio::test]
    async fn the_policy_refuses_a_removal_before_anything_runs() {
        use crate::platform::policy::{Key, Override};
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        record_outcome(&dirs, Dependency::Vagrant, Outcome::Installed);
        let _policy = Override::with(&[Key::DisableToolInstalls]);
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let mut lines = Vec::new();
        let err = uninstall_dependency(dirs.clone(), Dependency::Vagrant, |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::TurnedOffByPolicy { setting: "DisableToolInstalls".into() });
        assert_eq!(lines, [err.to_string()], "said on the removal's log");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(confirmed(load(&dirs).unwrap()).iter().any(|t| t.dependency == Dependency::Vagrant), "still remembered as installed");
    }

    /// Makes the recorded tools unreadable, as a damaged store would be.
    fn damage(dirs: &AppDirs) {
        crate::store::open(dirs)
            .unwrap()
            .write(|tx| {
                tx.execute("INSERT INTO installed_tools (dependency, at) VALUES ('no-such-tool', 1)", [])?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_damaged_record_is_never_written_over_nor_read_as_nothing_installed() {
        // Regression: a record that didn't read was taken for no tools, and the next install saved
        // that list over it: Cleanup stopped offering every tool recorded before, without a word.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        damage(&dirs);
        crate::testing::logs::take_warnings();
        record_outcome(&dirs, Dependency::Docker, Outcome::Installed);
        assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("couldn't be updated")), "said in the log");
        assert!(load(&dirs).is_err(), "left as it was");
        let listed = tauri::async_runtime::block_on(installed_tools(dirs.clone()));
        assert!(listed.is_empty());
        assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("can't be read")), "listing says why it is empty");
        let removal = tauri::async_runtime::block_on(uninstall_dependency(dirs, Dependency::Vagrant, |_| {}));
        assert!(removal.unwrap_err().to_string().contains("can't be read"));
    }

    #[test]
    fn records_round_trip_through_their_file_and_keep_one_entry_per_tool() {
        let dir = crate::testing::temp_dir("tools");
        let f = dir.join("data");
        assert!(load_at(&f).unwrap().is_empty());
        update_at(&f, |tools| {
            remember(tools, Dependency::Vagrant, Outcome::Installed);
            remember(tools, Dependency::Docker, Outcome::Installed);
        })
        .unwrap();
        update_at(&f, |tools| remember(tools, Dependency::Vagrant, Outcome::Installed)).unwrap();
        let back = load_at(&f).unwrap();
        assert_eq!(back.iter().map(|t| t.dependency).collect::<Vec<_>>(), [Dependency::Docker, Dependency::Vagrant]);
        assert!(back.iter().all(|t| t.at > 0));
        let json = serde_json::to_value(&back[0]).unwrap();
        assert_eq!(json["dependency"], "docker");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn updates_from_several_threads_each_keep_the_others_change() {
        let dir = crate::testing::temp_dir("tools-race");
        let f = dir.to_path_buf();
        let deps = [Dependency::Docker, Dependency::Vagrant, Dependency::Terraform, Dependency::Qemu];
        std::thread::scope(|s| {
            for dep in deps {
                let f = &f;
                s.spawn(move || update_at(f, |tools| remember(tools, dep, Outcome::Installed)).unwrap());
            }
        });
        assert_eq!(load_at(&f).unwrap().len(), deps.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_tool_missing_before_and_found_after_counts_as_ours() {
        // A tool the player already had must never be offered for removal, and an installer
        // that was only opened (macOS casks) is not an install yet.
        assert_eq!(outcome(true, true), Outcome::NotOurs);
        assert_eq!(outcome(true, false), Outcome::NotOurs);
        assert_eq!(outcome(false, true), Outcome::Installed);
        assert_eq!(outcome(false, false), Outcome::Pending);
        // WSL can't be told apart from its stub: never recorded.
        assert!(Dependency::Wsl.row().probe.is_none());
        assert!(Dependency::Docker.row().probe.is_some() && Dependency::Utm.row().mac_app.is_some());
    }

    #[test]
    fn pending_installs_count_once_found_and_expire_otherwise() {
        let mut tools = Vec::new();
        remember(&mut tools, Dependency::Docker, Outcome::Pending);
        remember(&mut tools, Dependency::Vagrant, Outcome::Pending);
        remember(&mut tools, Dependency::Terraform, Outcome::Installed);
        assert_eq!(confirmed(tools.clone()).iter().map(|t| t.dependency).collect::<Vec<_>>(), [Dependency::Terraform]);
        // The JSON the UI reads is unchanged for a confirmed record.
        assert!(serde_json::to_value(&tools[2]).unwrap().get("pending").is_none());
        // Docker's installer was finished: it counts now. Vagrant's is still open.
        let now = tools[0].at;
        settle(&mut tools, &[Dependency::Docker], now + 60);
        assert_eq!(confirmed(tools.clone()).len(), 2);
        assert!(tools.iter().any(|t| t.dependency == Dependency::Vagrant && t.pending));
        // A day later, the still-pending one is dropped.
        settle(&mut tools, &[], now + PENDING_FOR_SECS);
        assert_eq!(tools.iter().map(|t| t.dependency).collect::<Vec<_>>(), [Dependency::Docker, Dependency::Terraform]);
        // Re-opening an installer for a tool already ours keeps it ours.
        remember(&mut tools, Dependency::Docker, Outcome::Pending);
        assert!(confirmed(tools).iter().any(|t| t.dependency == Dependency::Docker));
    }

    #[test]
    fn the_virtualbox_uninstaller_only_runs_from_the_downloaded_installer() {
        // Any mounted /Volumes/VirtualBox* was trusted, and its tool run as admin; then the image
        // was globbed under `~` from the admin script, which runs as root (another home), after
        // Homebrew may have cleaned it up.
        let dmg = "/Users/ada/Library/Caches/Homebrew/downloads/abc--VirtualBox-7.2.20.dmg";
        let script = virtualbox_uninstall(dmg);
        assert!(!script.contains("/Volumes/VirtualBox") && !script.contains("exec ") && !script.contains('~'), "{script}");
        assert!(script.contains(&format!("'{dmg}'")) && script.contains("hdiutil detach"));
        assert!(crate::platform::sh_parses(&script), "{script}");
        // The path comes from Homebrew's answer, and only a quotable .dmg is accepted.
        assert_eq!(virtualbox_dmg_from(&format!("{dmg}\n")).unwrap(), dmg);
        for bad in ["", "/x/VirtualBox.pkg", "/x/it's.dmg", "/x/a\"b.dmg"] {
            assert!(virtualbox_dmg_from(bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_installer_image_path_with_spaces_or_shell_characters_is_single_quoted_in_the_admin_script() {
        // Homebrew's cache lives under the home folder, which can hold spaces; `$`, backticks and
        // `;` in a folder name must stay literal in the script run as root.
        let dmg = "/Users/a b/Library/Caches/Homebrew/$HOME`id`;x.dmg";
        assert_eq!(virtualbox_dmg_from(&format!("  {dmg}  \n")).unwrap(), dmg);
        let script = virtualbox_uninstall(dmg);
        assert!(script.contains(&format!("'{dmg}'")), "{script}");
        assert!(crate::platform::sh_parses(&script), "{script}");
        // Two lines (a second path appended) or another extension: not the image.
        assert!(virtualbox_dmg_from("/a.dmg\n/b.dmg").is_err());
        assert!(virtualbox_dmg_from("/a.DMG").is_err());
    }

    #[test]
    fn every_windows_tool_is_removed_through_winget() {
        let plan = |dep, dmg| plan_on(dep, WINDOWS, dmg);
        for dep in [
            Dependency::Docker,
            Dependency::Vagrant,
            Dependency::Terraform,
            Dependency::Virtualbox,
            Dependency::Awscli,
            Dependency::Azurecli,
            Dependency::Gcloud,
        ] {
            let steps = plan(dep, Some("/x/VirtualBox.dmg")).unwrap();
            assert_eq!(steps[0].program, "winget", "{dep:?}");
            assert!(steps[0].args.windows(2).any(|w| w == ["--source", "winget"]), "{dep:?}: {:?}", steps[0].args);
        }
        // Not in winget: its MSI removed by its product code.
        assert_eq!(plan(Dependency::VmwareUtility, None).unwrap()[0].program, "powershell");
        assert!(
            plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err()
                && plan(Dependency::Hyperv, None).is_err()
                && plan(Dependency::Utm, Some("/x/VirtualBox.dmg")).is_err()
                && plan(Dependency::Libvirt, Some("/x/VirtualBox.dmg")).is_err()
                && plan(Dependency::Qemu, None).is_err()
        );
    }

    #[test]
    fn every_macos_tool_has_a_way_out() {
        let plan = |dep, dmg| plan_on(dep, MAC, dmg);
        // Regression: VMware's utility, installed from HashiCorp's package without Homebrew, could
        // only be removed with Homebrew; so could every other tool from its vendor's package.
        let without = Host { packages: None, ..MAC };
        assert!(plan_on(Dependency::Terraform, without, None).err().is_some_and(|e| e.to_string().contains("Homebrew is needed")));
        for dep in [Dependency::VmwareUtility, Dependency::Vagrant, Dependency::Docker, Dependency::Utm] {
            assert!(plan_on(dep, without, None).is_ok(), "{dep:?}");
        }
        for dep in Dependency::ALL.into_iter().filter(|d| !matches!(d, Dependency::Wsl | Dependency::Libvirt | Dependency::Hyperv)) {
            assert!(!plan(dep, Some("/x/VirtualBox.dmg")).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err() && plan(Dependency::Libvirt, Some("/x/VirtualBox.dmg")).is_err());
    }

    #[test]
    fn every_linux_tool_has_a_way_out() {
        let plan = |dep, dmg| plan_on(dep, APT, dmg);
        for dep in Dependency::ALL.into_iter().filter(|d| !matches!(d, Dependency::Wsl | Dependency::Utm | Dependency::Hyperv)) {
            assert!(!plan(dep, Some("/x/VirtualBox.dmg")).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err() && plan(Dependency::Utm, Some("/x/VirtualBox.dmg")).is_err());
        // The packages holding the binaries the Machine page looks for go too, not only the
        // metapackages the install named.
        let script = |dep| plan(dep, Some("/x/VirtualBox.dmg")).unwrap()[0].args.join(" ");
        assert!(script(Dependency::Qemu).contains("'qemu-system*'"));
        assert!(script(Dependency::Libvirt).contains("libvirt-clients"));
    }

    #[test]
    fn a_tool_installed_through_dnf_is_removed_through_dnf() {
        for dep in [Dependency::Docker, Dependency::Vagrant, Dependency::Terraform, Dependency::Virtualbox, Dependency::Qemu, Dependency::Libvirt] {
            let script = plan_on(dep, DNF, None).unwrap()[0].args.join(" ");
            assert!(script.contains("dnf remove -y") && !script.contains("apt-get"), "{dep:?}: {script}");
        }
        // AWS CLI is a folder on every distribution.
        assert!(plan_on(Dependency::Awscli, DNF, None).unwrap()[0].args.join(" ").contains("rm -rf /usr/local/aws-cli"));
        assert!(plan_on(Dependency::Awscli, Host { packages: None, ..APT }, None).is_ok(), "a folder, whatever the distribution");
        // What the setup never installed through dnf has nothing to remove through it.
        for dep in [Dependency::Azurecli, Dependency::Gcloud, Dependency::VmwareUtility] {
            let err = plan_on(dep, DNF, None).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(err.contains("apt only"), "{dep:?}: {err}");
        }
        assert!(plan_on(Dependency::Vagrant, Host { packages: None, ..APT }, None).is_err());
    }
}
