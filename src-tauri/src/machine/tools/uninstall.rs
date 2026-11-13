//! Removing what Cyber CTF installed. Each successful assisted install is recorded, and Cleanup
//! offers to remove those tools only (a Docker or Vagrant the player had before is never
//! touched). Removal goes through the same trusted flows as install: the package manager,
//! the vendor's own uninstaller, or the OS admin prompt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::install::Dependency;
#[cfg(not(windows))]
use super::steps::step;
use super::steps::{Step, run_steps};
use crate::error::{Error, Result};
use crate::platform::AppDirs;

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

/// The command that proves `dep` is installed here, when there is one to run. WSL has none worth
/// trusting (`wsl.exe` ships with Windows whether WSL is set up or not), so it is never recorded.
fn presence_probe(dep: Dependency) -> Option<(&'static str, &'static [&'static str])> {
    Some(match dep {
        Dependency::Docker => ("docker", &["--version"]),
        Dependency::Vagrant => ("vagrant", &["--version"]),
        Dependency::Terraform => ("terraform", &["version"]),
        Dependency::Virtualbox => ("VBoxManage", &["--version"]),
        Dependency::Qemu => ("qemu-img", &["--version"]),
        Dependency::Libvirt => ("virsh", &["--version"]),
        Dependency::Awscli => ("aws", &["--version"]),
        Dependency::Azurecli => ("az", &["version"]),
        Dependency::Gcloud => ("gcloud", &["--version"]),
        Dependency::Utm | Dependency::Wsl | Dependency::VmwareUtility | Dependency::Hyperv => return None,
    })
}

/// The app bundle that proves `dep` is installed on macOS (also before its CLI is on PATH).
fn mac_app(dep: Dependency) -> Option<&'static str> {
    match dep {
        Dependency::Docker => Some("/Applications/Docker.app"),
        Dependency::Virtualbox => Some("/Applications/VirtualBox.app"),
        Dependency::Utm => Some("/Applications/UTM.app"),
        _ => None,
    }
}

/// Whether `dep` is installed on this machine; None when that can't be told (WSL).
pub async fn present(dep: Dependency) -> Option<bool> {
    // A service, not a program on PATH: what the machine check reads.
    if dep == Dependency::VmwareUtility {
        return Some(crate::runtime::providers::vmware_utility().await != crate::runtime::providers::UtilityState::Missing);
    }
    if cfg!(target_os = "macos") && mac_app(dep).is_some_and(|a| std::path::Path::new(a).exists()) {
        return Some(true);
    }
    match presence_probe(dep) {
        Some((program, args)) => Some(crate::platform::exec::run_read(program, args, None).await.is_ok()),
        None if cfg!(target_os = "macos") && mac_app(dep).is_some() => Some(false),
        None => None,
    }
}

/// Records the outcome of an install of `dependency`.
pub fn record_outcome(dirs: &AppDirs, dependency: Dependency, outcome: Outcome) {
    match outcome {
        Outcome::NotOurs => {}
        Outcome::Installed => update(dirs, |tools| remember(tools, dependency, false)),
        Outcome::Pending => update(dirs, |tools| remember(tools, dependency, true)),
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

fn records_file(dirs: &AppDirs) -> Option<PathBuf> {
    dirs.data_file("installed-tools.json")
}

/// The records; none without a data folder.
fn load(dirs: &AppDirs) -> std::io::Result<Vec<InstalledTool>> {
    records_file(dirs).map_or(Ok(Vec::new()), |f| load_at(&f))
}

/// [`load`], a record that can't be read logged and read as none (for listing them).
fn load_or_logged(dirs: &AppDirs) -> Vec<InstalledTool> {
    load(dirs).unwrap_or_else(|e| {
        log::warn!("the record of the tools Cyber CTF installed can't be read: {e}");
        Vec::new()
    })
}

/// The records in `f`: none when it is missing; an error when it can't be read or isn't a
/// record, which must not read as "nothing installed" (the next change would save that over it).
fn load_at(f: &Path) -> std::io::Result<Vec<InstalledTool>> {
    match std::fs::read(f) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Changes the records with `change`. One that can't be changed (unreadable, damaged, or it can't
/// be written) is left as it is and logged: Cleanup then doesn't offer to remove that tool.
fn update(dirs: &AppDirs, change: impl FnOnce(&mut Vec<InstalledTool>)) {
    if let Some(f) = records_file(dirs)
        && let Err(e) = update_at(&f, change)
    {
        log::warn!("the record of the tools Cyber CTF installed ({}) couldn't be updated: {e}", f.display());
    }
}

/// Changes the records in `f` with `change`, holding an OS lock on `<f>.lock` meanwhile: installs
/// and removals in different windows (machine setup, server setup, Settings) can end at the same
/// moment, and each must keep the other's change. Written whole (see `atomic_file`), so a crash
/// mid-write can't leave a half file.
fn update_at(f: &Path, change: impl FnOnce(&mut Vec<InstalledTool>)) -> std::io::Result<()> {
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(f.with_extension("lock"))?;
    lock.lock()?;
    let mut tools = load_at(f)?;
    change(&mut tools);
    crate::platform::atomic_file::write(f, serde_json::to_vec_pretty(&tools)?)
}

/// `tools` with `dependency` recorded once, as installed now (or waiting for its installer).
/// A tool already confirmed as ours stays confirmed.
fn remember(tools: &mut Vec<InstalledTool>, dependency: Dependency, pending: bool) {
    let pending = pending && !tools.iter().any(|t| t.dependency == dependency && !t.pending);
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
#[cfg(target_os = "macos")]
fn mac_admin(script: &str, note: &str) -> Step {
    let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
    Step {
        program: "osascript".into(),
        args: vec!["-e".into(), format!("do shell script \"{escaped}\" with administrator privileges")],
        note: Some(note.into()),
    }
}

/// VirtualBox's own uninstaller, run as admin, only ever from the installer disk image Homebrew
/// downloaded (`dmg`, attached here, then detached). Never from whatever volume happens to be
/// mounted as `/Volumes/VirtualBox*`: anyone can mount a disk image with that name and a
/// `VirtualBox_Uninstall.tool` of their own, which would then run as root. The image is found
/// before elevating: as root, `~` isn't the player's home.
#[cfg(any(target_os = "macos", test))]
fn virtualbox_uninstall(dmg: &str) -> String {
    format!(
        "m=$(hdiutil attach -nobrowse -noautoopen '{dmg}' | awk -F'\\t' '/\\/Volumes\\//{{print $NF; exit}}'); [ -n \"$m\" ] || exit 3; \
         \"$m/VirtualBox_Uninstall.tool\" --unattended; s=$?; hdiutil detach -quiet \"$m\"; exit $s"
    )
}

/// The VirtualBox installer image, from `brew --cache --cask virtualbox`'s output: a `.dmg` path
/// the admin script can quote.
#[cfg(any(target_os = "macos", test))]
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
#[cfg(target_os = "macos")]
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

/// How to remove `dep` on macOS: Homebrew's own packages, or the vendor's uninstaller.
#[cfg(target_os = "macos")]
fn macos_plan(dep: Dependency, virtualbox_dmg: Option<&str>) -> Result<Vec<Step>> {
    let brew = crate::platform::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is needed to remove this tool.".into()))?;
    Ok(match dep {
        Dependency::Terraform => vec![step(brew, &["uninstall", "hashicorp/tap/terraform"])],
        Dependency::Qemu => vec![step(brew, &["uninstall", "qemu"])],
        Dependency::Awscli => vec![step(brew, &["uninstall", "awscli"])],
        Dependency::Gcloud => vec![step(brew, &["uninstall", "--cask", "google-cloud-sdk"])],
        Dependency::Azurecli => vec![step(brew, &["uninstall", "--cask", "azure-cli-preview"])],
        // Installed from the vendors' own packages: removed the way their uninstallers do.
        Dependency::Vagrant => vec![mac_admin(
            "rm -rf /opt/vagrant /usr/local/bin/vagrant && pkgutil --forget com.vagrant.vagrant || true",
            "Removing Vagrant (macOS asks for your password)…",
        )],
        Dependency::Virtualbox => {
            let dmg = virtualbox_dmg.ok_or_else(|| Error::Invalid("VirtualBox's installer image is needed to run its uninstaller.".into()))?;
            vec![mac_admin(&virtualbox_uninstall(dmg), "Running VirtualBox's own uninstaller (macOS asks for your password)…")]
        }
        Dependency::Docker => vec![Step {
            program: "/Applications/Docker.app/Contents/MacOS/uninstall".into(),
            args: vec![],
            note: Some("Running Docker Desktop's own uninstaller…".into()),
        }],
        Dependency::Utm => vec![step("osascript", &["-e", "tell application \"Finder\" to delete POSIX file \"/Applications/UTM.app\""])],
        Dependency::VmwareUtility => {
            vec![mac_admin(super::vmware_utility::MAC_UNINSTALL, "Removing the Vagrant VMware Utility (macOS asks for your password)…")]
        }
        Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => return Err(super::install::not_here(dep)),
    })
}

/// How to remove `dep` on Windows: its winget package.
#[cfg(target_os = "windows")]
fn windows_plan(dep: Dependency) -> Result<Vec<Step>> {
    match (dep, super::install::winget_package(dep)) {
        (_, Some(id)) => Ok(vec![super::install::winget("uninstall", id, &[])]),
        (Dependency::VmwareUtility, None) => Ok(vec![super::vmware_utility::windows_uninstall()]),
        (Dependency::Wsl, None) => Err(Error::Invalid("Turn WSL off in Windows Features if you no longer need it.".into())),
        (Dependency::Hyperv, None) => Err(Error::Invalid("Turn Hyper-V off in Windows Features if you no longer need it.".into())),
        (_, None) => Err(super::install::not_here(dep)),
    }
}

/// How to remove `dep` on Linux: its packages through the package manager that installed them
/// (apt or dnf, as root through pkexec), or its folder.
#[cfg(target_os = "linux")]
fn linux_plan(dep: Dependency) -> Result<Vec<Step>> {
    linux_plan_with(dep, super::install::packages_here())
}

/// [`linux_plan`] on a machine with that package manager. Regression: a tool the setup had
/// installed through dnf on Fedora was removed with `apt-get remove`, which pkexec ran to
/// `apt-get: not found`, the record kept (the review of 2026-11-13).
#[cfg(target_os = "linux")]
fn linux_plan_with(dep: Dependency, packages: super::install::Packages) -> Result<Vec<Step>> {
    use super::install::Packages;
    let root = |script: &str| super::install::root(script.to_string());
    Ok(match (dep, packages) {
        (Dependency::Utm | Dependency::Wsl | Dependency::Hyperv, _) => return Err(super::install::not_here(dep)),
        (Dependency::Awscli, _) => vec![root("rm -rf /usr/local/aws-cli /usr/local/bin/aws /usr/local/bin/aws_completer")],
        (_, Packages::None) => return Err(super::install::without_packages(dep)),
        // Fedora: what the dnf install brought from the vendors' repositories and RPM Fusion; the
        // compiler, Ruby's headers and the shared tools (genisoimage) stay.
        (Dependency::Docker, Packages::Dnf) => vec![root("dnf remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin")],
        (Dependency::Vagrant, Packages::Dnf) => vec![root("dnf remove -y vagrant")],
        (Dependency::Terraform, Packages::Dnf) => vec![root("dnf remove -y terraform")],
        (Dependency::Virtualbox, Packages::Dnf) => vec![root("dnf remove -y VirtualBox akmod-VirtualBox 'kmod-VirtualBox*'")],
        (Dependency::Qemu, Packages::Dnf) => vec![root("dnf remove -y qemu-kvm qemu-img")],
        (Dependency::Libvirt, Packages::Dnf) => vec![root("dnf remove -y libvirt libvirt-daemon-kvm virt-install virt-manager libvirt-devel")],
        // The cloud CLIs and VMware's utility install through apt only: nothing of ours to remove.
        (_, Packages::Dnf) => return Err(super::install::apt_only(dep)),
        (Dependency::Docker, Packages::Apt) => vec![root("apt-get remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin")],
        (Dependency::Vagrant, Packages::Apt) => vec![root("apt-get remove -y vagrant")],
        (Dependency::Terraform, Packages::Apt) => vec![root("apt-get remove -y terraform")],
        (Dependency::Virtualbox, Packages::Apt) => vec![root("apt-get remove -y 'virtualbox*'")],
        // The install's metapackages (qemu-system, libvirt-daemon-system) leave the packages
        // they pulled in behind, with the binaries the Machine page looks for
        // (qemu-system-x86_64, virsh): those go too, or the tool still reads as installed.
        (Dependency::Qemu, Packages::Apt) => vec![root("apt-get remove -y 'qemu-system*' qemu-utils")],
        (Dependency::Libvirt, Packages::Apt) => vec![root("apt-get remove -y 'libvirt-daemon*' libvirt-clients virt-manager")],
        (Dependency::Azurecli, Packages::Apt) => vec![root("apt-get remove -y azure-cli")],
        // Google's apt package, and the per-user SDK earlier versions installed in its place.
        (Dependency::Gcloud, Packages::Apt) => vec![root("apt-get remove -y google-cloud-cli"), step("sh", &["-c", "rm -rf \"$HOME/google-cloud-sdk\""])],
        (Dependency::VmwareUtility, Packages::Apt) => vec![root("apt-get remove -y vagrant-vmware-utility")],
    })
}

/// How to remove `dep` on this OS. On macOS, VirtualBox's step needs its installer image
/// (`virtualbox_dmg`).
fn plan(dep: Dependency, virtualbox_dmg: Option<&str>) -> Result<Vec<Step>> {
    let _ = virtualbox_dmg;
    if !dep.runs_on(std::env::consts::OS) {
        return Err(super::install::not_here(dep));
    }
    #[cfg(target_os = "macos")]
    {
        macos_plan(dep, virtualbox_dmg)
    }
    #[cfg(target_os = "windows")]
    {
        windows_plan(dep)
    }
    #[cfg(target_os = "linux")]
    {
        linux_plan(dep)
    }
}

/// Removes a tool Cyber CTF installed, streaming the output, then forgets it.
pub async fn uninstall_dependency(dirs: AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    super::install::installs_allowed(&mut on_line)?;
    let recorded = load(&dirs).map_err(|e| Error::Invalid(format!("the record of the tools Cyber CTF installed can't be read: {e}")))?;
    if !confirmed(recorded).iter().any(|t| t.dependency == dependency) {
        return Err(Error::Invalid("Cyber CTF didn't install this tool, so it won't remove it.".into()));
    }
    #[cfg(target_os = "macos")]
    let dmg = match dependency {
        Dependency::Virtualbox => {
            let brew = crate::platform::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is needed to remove this tool.".into()))?;
            Some(virtualbox_dmg(brew, &mut on_line).await?)
        }
        _ => None,
    };
    #[cfg(not(target_os = "macos"))]
    let dmg: Option<String> = None;
    run_steps(&plan(dependency, dmg.as_deref())?, &mut on_line).await?;
    update(&dirs, |tools| tools.retain(|t| t.dependency != dependency));
    on_line("Removed.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_this_os_doesnt_have_is_refused_as_the_install_refuses_it() {
        for dep in Dependency::ALL.into_iter().filter(|d| !d.runs_on(std::env::consts::OS)) {
            assert_eq!(plan(dep, None).err().map(|e| e.to_string()), Some(super::super::install::not_here(dep).to_string()));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_removal_runs_as_root_as_an_install_does() {
        // Regression: removals built their own pkexec line, without the proxy installs hand it.
        let steps = linux_plan(Dependency::Vagrant).unwrap();
        assert_eq!(steps[0].program, "pkexec");
        assert_eq!(steps[0].args.first().map(String::as_str), Some("env"));
        assert_eq!(steps[0].args.last().map(String::as_str), Some("apt-get remove -y vagrant"));
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

    #[test]
    fn a_record_that_cant_be_written_is_said_in_the_log() {
        // Regression: a failed write of the record was dropped, and Cleanup silently stopped
        // offering to remove the tool.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let f = records_file(&dirs).unwrap();
        std::fs::create_dir_all(&f).unwrap();
        crate::testing::logs::take_warnings();
        record_outcome(&dirs, Dependency::Vagrant, Outcome::Installed);
        let logged = crate::testing::logs::take_warnings();
        assert!(logged.iter().any(|w| w.starts_with("the record of the tools Cyber CTF installed")), "{logged:?}");
    }

    #[test]
    fn a_damaged_record_is_never_written_over_nor_read_as_nothing_installed() {
        // Regression: a record that didn't parse read as no tools, and the next install saved
        // that list over it: Cleanup stopped offering every tool recorded before, without a word.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let f = records_file(&dirs).unwrap();
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "[{\"dependency\":\"vagrant\",").unwrap();
        crate::testing::logs::take_warnings();
        record_outcome(&dirs, Dependency::Docker, Outcome::Installed);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[{\"dependency\":\"vagrant\",", "left as it was");
        assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("couldn't be updated")));
        assert_eq!(load(&dirs).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let listed = tauri::async_runtime::block_on(installed_tools(dirs.clone()));
        assert!(listed.is_empty());
        assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("can't be read")), "listing says why it is empty");
        let removal = tauri::async_runtime::block_on(uninstall_dependency(dirs, Dependency::Vagrant, |_| {}));
        assert!(removal.unwrap_err().to_string().contains("can't be read"));
    }

    #[test]
    fn records_round_trip_through_their_file_and_keep_one_entry_per_tool() {
        let dir = crate::testing::temp_dir("tools");
        let f = dir.join("data").join("installed-tools.json");
        assert!(load_at(&f).unwrap().is_empty());
        update_at(&f, |tools| {
            remember(tools, Dependency::Vagrant, false);
            remember(tools, Dependency::Docker, false);
        })
        .unwrap();
        update_at(&f, |tools| remember(tools, Dependency::Vagrant, false)).unwrap();
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
        let f = dir.join("installed-tools.json");
        let deps = [Dependency::Docker, Dependency::Vagrant, Dependency::Terraform, Dependency::Qemu];
        std::thread::scope(|s| {
            for dep in deps {
                let f = &f;
                s.spawn(move || update_at(f, |tools| remember(tools, dep, false)).unwrap());
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
        assert!(presence_probe(Dependency::Wsl).is_none());
        assert!(presence_probe(Dependency::Docker).is_some() && mac_app(Dependency::Utm).is_some());
    }

    #[test]
    fn pending_installs_count_once_found_and_expire_otherwise() {
        let mut tools = Vec::new();
        remember(&mut tools, Dependency::Docker, true);
        remember(&mut tools, Dependency::Vagrant, true);
        remember(&mut tools, Dependency::Terraform, false);
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
        remember(&mut tools, Dependency::Docker, true);
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

    #[cfg(target_os = "windows")]
    #[test]
    fn every_windows_tool_is_removed_through_winget() {
        for dep in [
            Dependency::Docker,
            Dependency::Vagrant,
            Dependency::Terraform,
            Dependency::Virtualbox,
            Dependency::Qemu,
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
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_macos_tool_has_a_way_out() {
        if crate::platform::brew_bin().is_none() {
            return; // CI without Homebrew: the brew-based plans can't be built.
        }
        for dep in Dependency::ALL.into_iter().filter(|d| !matches!(d, Dependency::Wsl | Dependency::Libvirt | Dependency::Hyperv)) {
            assert!(!plan(dep, Some("/x/VirtualBox.dmg")).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err() && plan(Dependency::Libvirt, Some("/x/VirtualBox.dmg")).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_linux_tool_has_a_way_out() {
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_tool_installed_through_dnf_is_removed_through_dnf() {
        use super::super::install::Packages;
        for dep in [Dependency::Docker, Dependency::Vagrant, Dependency::Terraform, Dependency::Virtualbox, Dependency::Qemu, Dependency::Libvirt] {
            let script = linux_plan_with(dep, Packages::Dnf).unwrap()[0].args.join(" ");
            assert!(script.contains("dnf remove -y") && !script.contains("apt-get"), "{dep:?}: {script}");
        }
        // AWS CLI is a folder on every distribution.
        assert!(linux_plan_with(Dependency::Awscli, Packages::Dnf).unwrap()[0].args.join(" ").contains("rm -rf /usr/local/aws-cli"));
        // What the setup never installed through dnf has nothing to remove through it.
        for dep in [Dependency::Azurecli, Dependency::Gcloud, Dependency::VmwareUtility] {
            let err = linux_plan_with(dep, Packages::Dnf).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(err.contains("apt only"), "{dep:?}: {err}");
        }
        assert!(linux_plan_with(Dependency::Vagrant, Packages::None).is_err());
    }
}
