//! Removing what Cyber CTF installed. What each successful assisted install added is recorded
//! (isothrum-machine reads it: [`Installed`](isothrum_machine::install::Installed)), never what was
//! there before it, and Cleanup removes only that: a Docker or Vagrant the player had before is
//! never touched, nor a QEMU the player installed whose helpers an install added. Removal goes
//! through the same trusted flows as install: the package manager, the vendor's own uninstaller,
//! or the OS admin prompt.

use std::path::Path;

use isothrum_machine::install::record::{self, InstalledTool, removable};
use isothrum_machine::install::{Installed as Added, Item};
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

/// What of `dep`'s install is on this machine now (isothrum-machine's
/// [`there`](isothrum_machine::install::there)); `None` when that can't be told.
async fn there(dep: Dependency) -> Option<Vec<Item>> {
    isothrum_machine::install::there(&crate::runtime::providers::machine_setup().await, dep).await.map(|items| items.into_iter().collect())
}

/// Records what an install of `dependency` added (isothrum-machine's
/// [`remember`](record::remember)): nothing when it added nothing.
pub fn record_installed(dirs: &AppDirs, dependency: Dependency, installed: &Added) {
    if !installed.is_empty() {
        update(dirs, |tools| record::remember(tools, dependency, installed, crate::platform::clock::unix_secs()));
    }
}

/// Re-checks what installs left waiting (an installer only opened): run when the tools are listed.
async fn confirm_pending(dirs: &AppDirs) {
    let waiting: Vec<Dependency> = load_or_logged(dirs).into_iter().filter(|t| !t.waiting.is_empty()).map(|t| t.dependency).collect();
    if waiting.is_empty() {
        return;
    }
    let mut found = Vec::new();
    for dep in waiting {
        if let Some(items) = there(dep).await {
            found.push((dep, items));
        }
    }
    let now = crate::platform::clock::unix_secs();
    update(dirs, |tools| record::settle(tools, &found, now));
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

/// The tools Cyber CTF added something of on this machine (and hasn't removed since). Installs
/// that only opened the vendor's installer count once the tool is found.
pub async fn installed_tools(dirs: AppDirs) -> Vec<Installed> {
    confirm_pending(&dirs).await;
    removable(load_or_logged(&dirs)).into_iter().map(|t| Installed { dependency: t.dependency, name: super::install::name(t.dependency), at: t.at }).collect()
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

/// How to remove what an install added of `dep` (`added`, as on record) on `host`, and nothing
/// else: isothrum-machine's [`removal`](isothrum_machine::install::removal) (the package manager's
/// removal of those packages, or the vendor's uninstaller when the install added the tool
/// itself). On macOS, VirtualBox's step needs its installer image (`virtualbox_dmg`).
pub(super) fn plan_on(dep: Dependency, host: &Host, added: &[Item], virtualbox_dmg: Option<&str>) -> Result<super::steps::Plan> {
    if !super::install::runs_on(dep, host.os) {
        return Err(super::install::not_here(dep, host.os));
    }
    isothrum_machine::install::removal(dep, host, added, super::install::elevation(), virtualbox_dmg).map_err(super::install::refusal)
}

/// Why a removal of a tool Cyber CTF added nothing of is refused.
pub(super) fn not_ours() -> Error {
    Error::Invalid("Cyber CTF didn't install this tool, so it won't remove it.".into())
}

/// Removes what Cyber CTF's installs added of a tool, streaming the output, then forgets it.
pub async fn uninstall_dependency(dirs: AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    super::install::installs_allowed(&mut on_line)?;
    let recorded = load(&dirs).map_err(|e| Error::Invalid(format!("the record of the tools Cyber CTF installed can't be read: {e}")))?;
    let Some(added) = removable(recorded).into_iter().find(|t| t.dependency == dependency).map(|t| t.added) else {
        return Err(not_ours());
    };
    let host = crate::runtime::providers::host().await;
    let dmg = match (host.os, host.packages, dependency) {
        (HostOs::Macos, Some(Packages::Homebrew(brew)), Dependency::Virtualbox) if added.contains(&Item::Tool(dependency)) => {
            Some(virtualbox_dmg(brew, &mut on_line).await?)
        }
        _ => None,
    };
    super::steps::run_plan(&plan_on(dependency, &host, &added, dmg.as_deref())?, &mut on_line).await?;
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
        record_installed(&dirs, Dependency::Vagrant, &tool(Dependency::Vagrant));
        let _policy = Override::with(&[Key::DisableToolInstalls]);
        let tools = crate::runtime::providers::testing::fake(isothrum_runner::fake::FakeProcesses::script().build());
        let mut lines = Vec::new();
        let err = uninstall_dependency(dirs.clone(), Dependency::Vagrant, |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::TurnedOffByPolicy { setting: "DisableToolInstalls".into() });
        assert_eq!(lines, [err.to_string()], "said on the removal's log");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(removable(load(&dirs).unwrap()).iter().any(|t| t.dependency == Dependency::Vagrant), "still remembered as installed");
    }

    /// What an install that added the tool `dep` itself reports.
    fn tool(dep: Dependency) -> Added {
        Added::new(vec![Item::Tool(dep)], vec![])
    }

    /// Makes the recorded tools unreadable, as a damaged store would be.
    fn damage(dirs: &AppDirs) {
        crate::store::open(dirs)
            .unwrap()
            .write(|tx| {
                tx.execute("INSERT INTO installed_tools (dependency, at, added) VALUES ('no-such-tool', 1, '[]')", [])?;
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
        record_installed(&dirs, Dependency::Docker, &tool(Dependency::Docker));
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
            record::remember(tools, Dependency::Vagrant, &tool(Dependency::Vagrant), 1);
            record::remember(tools, Dependency::Docker, &tool(Dependency::Docker), 1);
        })
        .unwrap();
        update_at(&f, |tools| record::remember(tools, Dependency::Vagrant, &tool(Dependency::Vagrant), 1)).unwrap();
        let back = load_at(&f).unwrap();
        assert_eq!(back.iter().map(|t| t.dependency).collect::<Vec<_>>(), [Dependency::Docker, Dependency::Vagrant]);
        assert!(back.iter().all(|t| t.at > 0));
        let json = serde_json::to_value(&back[0]).unwrap();
        assert_eq!(json["dependency"], "docker");
        assert_eq!(back[1].added, [Item::Tool(Dependency::Vagrant)], "what it added, read back");
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
                s.spawn(move || update_at(f, |tools| record::remember(tools, dep, &tool(dep), 1)).unwrap());
            }
        });
        assert_eq!(load_at(&f).unwrap().len(), deps.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tool_the_player_installed_stays_when_an_install_only_added_its_helpers() {
        // QEMU installed by hand, its VMs' helpers added by the setup: Cleanup removes those, and
        // never QEMU itself.
        let linux = host_on(HostOs::Linux, isothrum_machine::Arch::Amd64, Some(Packages::Linux(isothrum_machine::LinuxPackages::Apt)));
        let added = [Item::Package("genisoimage".into()), Item::Package("virtiofsd".into())];
        let steps = plan_on(Dependency::Qemu, &linux, &added, None).unwrap();
        let script = steps[0].args.last().unwrap();
        assert!(script.ends_with("apt-get remove -y genisoimage virtiofsd"), "{script}");
        assert!(!script.contains("qemu-system") && !script.contains("qemu-utils"), "{script}");
        // Nothing on record of a tool, or only another's: refused before any step.
        for added in [vec![], vec![Item::Package("qemu-system-x86".into())], vec![Item::Tool(Dependency::Qemu)]] {
            let refused = plan_on(Dependency::Libvirt, &linux, &added, None).unwrap_err();
            assert_eq!(refused.to_string(), not_ours().to_string(), "{added:?}");
        }
        let mac = plan_on(Dependency::Qemu, &MAC, &[], None).unwrap_err();
        assert_eq!(mac.to_string(), not_ours().to_string());
    }

    #[test]
    fn every_windows_tool_is_removed_through_winget() {
        let plan = |dep, dmg| plan_on(dep, &WINDOWS, &[Item::Tool(dep)], dmg);
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
        let plan = |dep, dmg| plan_on(dep, &MAC, &[Item::Tool(dep)], dmg);
        // Regression: VMware's utility, installed from HashiCorp's package without Homebrew, could
        // only be removed with Homebrew; so could every other tool from its vendor's package.
        let without = Host { packages: None, ..MAC.clone() };
        let whole = |dep| [Item::Tool(dep)];
        assert!(
            plan_on(Dependency::Terraform, &without, &whole(Dependency::Terraform), None).err().is_some_and(|e| e.to_string().contains("Homebrew is needed"))
        );
        for dep in [Dependency::VmwareUtility, Dependency::Vagrant, Dependency::Docker, Dependency::Utm] {
            assert!(plan_on(dep, &without, &whole(dep), None).is_ok(), "{dep:?}");
        }
        for dep in Dependency::ALL.into_iter().filter(|d| !matches!(d, Dependency::Wsl | Dependency::Libvirt | Dependency::Hyperv)) {
            assert!(!plan(dep, Some("/x/VirtualBox.dmg")).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl, Some("/x/VirtualBox.dmg")).is_err() && plan(Dependency::Libvirt, Some("/x/VirtualBox.dmg")).is_err());
    }
}
