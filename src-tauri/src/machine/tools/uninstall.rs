//! Removing what Cyber CTF installed. Each successful assisted install is recorded, and Cleanup
//! offers to remove those tools only (a Docker or Vagrant the player had before is never
//! touched). Removal goes through the same trusted flows as install: the package manager,
//! the vendor's own uninstaller, or the OS admin prompt.

use std::path::Path;

use isothrum_machine::install::record::{self, InstalledTool, Outcome, confirmed};
use isothrum_machine::{Host, HostOs, Packages};
use serde::Serialize;

use super::install::Dependency;
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// A tool Cyber CTF installed, as Settings lists it: the record, and its name here.
#[derive(Serialize, Clone, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "InstalledTool")]
pub struct Installed {
    /// The tool.
    pub dependency: Dependency,
    /// Its name on this OS, as the setup names it.
    pub name: &'static str,
    /// When Cyber CTF installed it, in seconds since the Unix epoch.
    pub at: u64,
}

/// Whether `dep` is installed on this machine; None when that can't be told (WSL): as
/// isothrum-machine finds it.
pub async fn present(dep: Dependency) -> Option<bool> {
    isothrum_machine::install::present(&crate::runtime::providers::machine(), dep).await
}

/// Records the outcome of an install of `dependency` (isothrum-machine's
/// [`remember`](record::remember)).
pub fn record_outcome(dirs: &AppDirs, dependency: Dependency, outcome: Outcome) {
    if outcome != Outcome::NotOurs {
        update(dirs, |tools| record::remember(tools, dependency, outcome, crate::platform::clock::unix_secs()));
    }
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
        record::settle(tools, &found, now);
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

/// The tools Cyber CTF installed on this machine (and hasn't removed since). Installs that only
/// opened the vendor's installer count once the tool is found.
pub async fn installed_tools(dirs: AppDirs) -> Vec<Installed> {
    confirm_pending(&dirs).await;
    confirmed(load_or_logged(&dirs)).into_iter().map(|t| Installed { dependency: t.dependency, name: super::install::name(t.dependency), at: t.at }).collect()
}

/// The VirtualBox installer image Homebrew (`brew`) keeps, which its uninstaller runs from
/// (isothrum-machine's [`virtualbox_image`](isothrum_machine::install::macos::virtualbox_image)),
/// fetched again first as the player.
async fn virtualbox_dmg(brew: &str, on_line: &mut impl FnMut(String)) -> Result<String> {
    use isothrum_machine::install::macos::{ImageMissing, virtualbox_image};
    let cx = crate::runtime::providers::machine_setup().await;
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
    isothrum_machine::install::macos::uninstall(dep, packages, virtualbox_dmg).map_err(super::install::refusal)
}

/// The removal on Windows: isothrum-machine's winget packages and VMware's utility's MSI.
fn windows_plan(dep: Dependency) -> Result<super::steps::Plan> {
    isothrum_machine::install::windows::uninstall(dep).map_err(super::install::refusal)
}

/// How to remove `dep` on Linux: its packages through the package manager that installed them
/// (apt or dnf, as root through pkexec), or its folder. Regression: a tool the setup had
/// installed through dnf on Fedora was removed with `apt-get remove`, which pkexec ran to
/// The removal on Linux: isothrum-machine's root script, through the package manager that
/// installed the tool.
fn linux_plan(dep: Dependency, packages: Option<Packages>) -> Result<super::steps::Plan> {
    isothrum_machine::install::linux::uninstall(dep, packages, &super::install::linux_root()).map_err(super::install::refusal)
}

/// How to remove `dep` on `host`. On macOS, VirtualBox's step needs its installer image
/// (`virtualbox_dmg`).
pub(super) fn plan_on(dep: Dependency, host: &Host, virtualbox_dmg: Option<&str>) -> Result<super::steps::Plan> {
    if !super::install::runs_on(dep, host.os) {
        return Err(super::install::not_here(dep, host.os));
    }
    match host.os {
        HostOs::Macos => macos_plan(dep, host.packages, virtualbox_dmg),
        HostOs::Windows => windows_plan(dep),
        HostOs::Linux => linux_plan(dep, host.packages),
    }
}

/// Removes a tool Cyber CTF installed, streaming the output, then forgets it.
pub async fn uninstall_dependency(dirs: AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    super::install::installs_allowed(&mut on_line)?;
    let recorded = load(&dirs).map_err(|e| Error::Invalid(format!("the record of the tools Cyber CTF installed can't be read: {e}")))?;
    if !confirmed(recorded).iter().any(|t| t.dependency == dependency) {
        return Err(Error::Invalid("Cyber CTF didn't install this tool, so it won't remove it.".into()));
    }
    let host = crate::runtime::providers::host().await;
    let dmg = match (host.os, host.packages, dependency) {
        (HostOs::Macos, Some(Packages::Homebrew(brew)), Dependency::Virtualbox) => Some(virtualbox_dmg(brew, &mut on_line).await?),
        _ => None,
    };
    super::steps::run_plan(&plan_on(dependency, &host, dmg.as_deref())?, &mut on_line).await?;
    update(&dirs, |tools| tools.retain(|t| t.dependency != dependency));
    on_line("Removed.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::runtime::providers::testing::host_on;

    const BREW: &str = "/opt/homebrew/bin/brew";
    static MAC: LazyLock<Host> = LazyLock::new(|| host_on(HostOs::Macos, isothrum_machine::Arch::Arm64, Some(Packages::Homebrew(BREW))));
    static WINDOWS: LazyLock<Host> = LazyLock::new(|| host_on(HostOs::Windows, isothrum_machine::Arch::Amd64, Some(Packages::Winget)));

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
        assert_eq!(load_at(&f).unwrap(), []);
        update_at(&f, |tools| {
            record::remember(tools, Dependency::Vagrant, Outcome::Installed, 1);
            record::remember(tools, Dependency::Docker, Outcome::Installed, 1);
        })
        .unwrap();
        update_at(&f, |tools| record::remember(tools, Dependency::Vagrant, Outcome::Installed, 1)).unwrap();
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
                s.spawn(move || update_at(f, |tools| record::remember(tools, dep, Outcome::Installed, 1)).unwrap());
            }
        });
        assert_eq!(load_at(&f).unwrap().len(), deps.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_windows_tool_is_removed_through_winget() {
        let plan = |dep, dmg| plan_on(dep, &WINDOWS, dmg);
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
        let plan = |dep, dmg| plan_on(dep, &MAC, dmg);
        // Regression: VMware's utility, installed from HashiCorp's package without Homebrew, could
        // only be removed with Homebrew; so could every other tool from its vendor's package.
        let without = Host { packages: None, ..MAC.clone() };
        assert!(plan_on(Dependency::Terraform, &without, None).err().is_some_and(|e| e.to_string().contains("Homebrew is needed")));
        for dep in [Dependency::VmwareUtility, Dependency::Vagrant, Dependency::Docker, Dependency::Utm] {
            assert!(plan_on(dep, &without, None).is_ok(), "{dep:?}");
        }
        for dep in Dependency::ALL.into_iter().filter(|d| !matches!(d, Dependency::Wsl | Dependency::Libvirt | Dependency::Hyperv)) {
            assert!(!plan(dep, Some("/x/VirtualBox.dmg")).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err() && plan(Dependency::Libvirt, Some("/x/VirtualBox.dmg")).is_err());
    }
}
