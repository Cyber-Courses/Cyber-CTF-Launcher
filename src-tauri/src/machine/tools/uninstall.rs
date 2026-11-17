//! Removing what Cyber CTF installed. Each successful assisted install is recorded, and Cleanup
//! offers to remove those tools only (a Docker or Vagrant the player had before is never
//! touched). Removal goes through the same trusted flows as install: the package manager,
//! the vendor's own uninstaller, or the OS admin prompt.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::install::Dependency;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::host::{Host, Os, Packages};

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

/// The VirtualBox installer image Homebrew (`brew`) keeps, which its uninstaller runs from
/// (isothrum-machine's [`virtualbox_image`](isothrum_machine::install::macos::virtualbox_image)),
/// fetched again first as the player.
async fn virtualbox_dmg(brew: &str, on_line: &mut impl FnMut(String)) -> Result<String> {
    use isothrum_machine::install::macos::{ImageMissing, virtualbox_image};
    let cx = crate::runtime::providers::machine_check();
    let mut lines = Vec::new();
    let found = virtualbox_image(&cx, brew, &mut |line: &str| lines.push(line.to_owned())).await;
    lines.into_iter().for_each(&mut *on_line);
    found.map_err(|missing| match missing {
        ImageMissing::NotThere(dmg) => Error::Invalid(format!(
            "VirtualBox's installer image isn't there ({dmg}), so its uninstaller can't be run. Run `brew fetch --cask virtualbox`, then try again."
        )),
        ImageMissing::Failed(failed) => super::steps::step_failed(failed, true),
        _ => Error::Invalid("Homebrew didn't say where VirtualBox's installer image is, so its uninstaller can't be run.".into()),
    })
}

/// The removal on macOS: isothrum-machine's (Homebrew's packages, or the vendor's uninstaller).
fn macos_plan(dep: Dependency, packages: Option<Packages>, virtualbox_dmg: Option<&str>) -> Result<super::steps::Plan> {
    isothrum_machine::install::macos::uninstall(dep.machine(), packages.map(Packages::isothrum), virtualbox_dmg).map_err(super::install::refusal)
}

/// The removal on Windows: isothrum-machine's winget packages and VMware's utility's MSI.
fn windows_plan(dep: Dependency) -> Result<super::steps::Plan> {
    isothrum_machine::install::windows::uninstall(dep.machine()).map_err(super::install::refusal)
}

/// How to remove `dep` on Linux: its packages through the package manager that installed them
/// (apt or dnf, as root through pkexec), or its folder. Regression: a tool the setup had
/// installed through dnf on Fedora was removed with `apt-get remove`, which pkexec ran to
/// The removal on Linux: isothrum-machine's root script, through the package manager that
/// installed the tool.
fn linux_plan(dep: Dependency, packages: Option<Packages>) -> Result<super::steps::Plan> {
    isothrum_machine::install::linux::uninstall(dep.machine(), packages.map(Packages::isothrum), &super::install::linux_root()).map_err(super::install::refusal)
}

/// How to remove `dep` on `host`. On macOS, VirtualBox's step needs its installer image
/// (`virtualbox_dmg`).
pub(super) fn plan_on(dep: Dependency, host: Host, virtualbox_dmg: Option<&str>) -> Result<super::steps::Plan> {
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
    super::steps::run_plan(&plan_on(dependency, host, dmg.as_deref())?, &mut on_line).await?;
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
}
