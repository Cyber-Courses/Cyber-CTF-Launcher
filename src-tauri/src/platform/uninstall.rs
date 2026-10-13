//! Removing what Cyber CTF installed. Each successful assisted install is recorded, and Cleanup
//! offers to remove those tools only (a Docker or Vagrant the player had before is never
//! touched). Removal goes through the same trusted flows as install: the package manager,
//! the vendor's own uninstaller, or the OS admin prompt.

use std::path::PathBuf;

use crate::AppHandle;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri::ipc::Channel;

use super::install::Dependency;
use super::steps::{Step, channel_log, run_steps, step};
use crate::error::{Error, Result};

#[derive(Serialize, Deserialize, Clone, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
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

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

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
        Dependency::Utm | Dependency::Wsl => return None,
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
    if cfg!(target_os = "macos") && mac_app(dep).is_some_and(|a| std::path::Path::new(a).exists()) {
        return Some(true);
    }
    match presence_probe(dep) {
        Some((program, args)) => Some(crate::exec::run_read(program, args, None).await.is_ok()),
        None if cfg!(target_os = "macos") && mac_app(dep).is_some() => Some(false),
        None => None,
    }
}

/// Records the outcome of an install of `dependency`.
pub fn record_outcome(app: &AppHandle, dependency: Dependency, outcome: Outcome) {
    match outcome {
        Outcome::NotOurs => {}
        Outcome::Installed => update(app, |tools| remember(tools, dependency, false)),
        Outcome::Pending => update(app, |tools| remember(tools, dependency, true)),
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
async fn confirm_pending(app: &AppHandle) {
    let waiting: Vec<Dependency> = load(app).into_iter().filter(|t| t.pending).map(|t| t.dependency).collect();
    if waiting.is_empty() {
        return;
    }
    let mut found = Vec::new();
    for dep in waiting {
        if present(dep).await == Some(true) {
            found.push(dep);
        }
    }
    let now = now_secs();
    update(app, |tools| {
        settle(tools, &found, now);
    });
}

fn records_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("installed-tools.json"))
}

fn load(app: &AppHandle) -> Vec<InstalledTool> {
    records_file(app).map(|f| load_at(&f)).unwrap_or_default()
}

/// The records in `f`; none when it is missing or unreadable.
fn load_at(f: &std::path::Path) -> Vec<InstalledTool> {
    std::fs::read(f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Written whole through a temporary file and a rename, so a crash mid-write can't leave a
/// half file that reads as "nothing installed".
fn save(app: &AppHandle, tools: &[InstalledTool]) {
    if let Some(f) = records_file(app) {
        save_at(&f, tools);
    }
}

fn save_at(f: &std::path::Path, tools: &[InstalledTool]) {
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(tools) {
        let tmp = f.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, f);
        }
    }
}

/// Serialises the read-modify-writes of the records: installs and removals in different windows
/// (machine setup, server setup, Settings) can end at the same moment, and each must keep the
/// other's change.
static RECORDS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn update(app: &AppHandle, f: impl FnOnce(&mut Vec<InstalledTool>)) {
    let _guard = RECORDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut tools = load(app);
    f(&mut tools);
    save(app, &tools);
}

/// `tools` with `dependency` recorded once, as installed now (or waiting for its installer).
/// A tool already confirmed as ours stays confirmed.
fn remember(tools: &mut Vec<InstalledTool>, dependency: Dependency, pending: bool) {
    let pending = pending && !tools.iter().any(|t| t.dependency == dependency && !t.pending);
    tools.retain(|t| t.dependency != dependency);
    tools.push(InstalledTool { dependency, at: now_secs(), pending });
}

/// The confirmed records only.
fn confirmed(tools: Vec<InstalledTool>) -> Vec<InstalledTool> {
    tools.into_iter().filter(|t| !t.pending).collect()
}

/// The tools Cyber CTF installed on this machine (and hasn't removed since). Installs that only
/// opened the vendor's installer count once the tool is found.
#[tauri::command]
pub async fn installed_tools(app: AppHandle) -> Vec<InstalledTool> {
    confirm_pending(&app).await;
    confirmed(load(&app))
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
/// downloaded for the install (attached here, then detached). Never from whatever volume happens
/// to be mounted as `/Volumes/VirtualBox*`: anyone can mount a disk image with that name and a
/// `VirtualBox_Uninstall.tool` of their own, which would then run as root.
#[cfg(any(target_os = "macos", test))]
const VIRTUALBOX_UNINSTALL: &str = "f=$(ls -t ~/Library/Caches/Homebrew/downloads/*VirtualBox*.dmg 2>/dev/null | head -n 1); [ -n \"$f\" ] || exit 3; \
     m=$(hdiutil attach -nobrowse -noautoopen \"$f\" | awk -F'\\t' '/\\/Volumes\\//{print $NF; exit}'); [ -n \"$m\" ] || exit 3; \
     \"$m/VirtualBox_Uninstall.tool\" --unattended; s=$?; hdiutil detach -quiet \"$m\"; exit $s";

/// How to remove `dep` on this OS.
fn plan(dep: Dependency) -> Result<Vec<Step>> {
    #[cfg(target_os = "macos")]
    {
        let brew = super::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is needed to remove this tool.".into()))?;
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
            Dependency::Virtualbox => vec![mac_admin(VIRTUALBOX_UNINSTALL, "Running VirtualBox's own uninstaller (macOS asks for your password)…")],
            Dependency::Docker => vec![Step {
                program: "/Applications/Docker.app/Contents/MacOS/uninstall".into(),
                args: vec![],
                note: Some("Running Docker Desktop's own uninstaller…".into()),
            }],
            Dependency::Utm => vec![step("osascript", &["-e", "tell application \"Finder\" to delete POSIX file \"/Applications/UTM.app\""])],
            Dependency::Libvirt | Dependency::Wsl => return Err(Error::Invalid("Not installed on macOS.".into())),
        })
    }
    #[cfg(target_os = "windows")]
    {
        let id = match dep {
            Dependency::Docker => "Docker.DockerDesktop",
            Dependency::Vagrant => "Hashicorp.Vagrant",
            Dependency::Terraform => "Hashicorp.Terraform",
            Dependency::Virtualbox => "Oracle.VirtualBox",
            Dependency::Qemu => "SoftwareFreedomConservancy.QEMU",
            Dependency::Awscli => "Amazon.AWSCLI",
            Dependency::Azurecli => "Microsoft.AzureCLI",
            Dependency::Gcloud => "Google.CloudSDK",
            Dependency::Wsl => return Err(Error::Invalid("Turn WSL off in Windows Features if you no longer need it.".into())),
            Dependency::Utm | Dependency::Libvirt => return Err(Error::Invalid("Not installed on Windows.".into())),
        };
        Ok(vec![step("winget", &["uninstall", "-e", "--id", id])])
    }
    #[cfg(target_os = "linux")]
    {
        let root = |script: &str| step("pkexec", &["sh", "-c", script]);
        Ok(match dep {
            Dependency::Docker => vec![root("apt-get remove -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin")],
            Dependency::Vagrant => vec![root("apt-get remove -y vagrant")],
            Dependency::Terraform => vec![root("apt-get remove -y terraform")],
            Dependency::Virtualbox => vec![root("apt-get remove -y 'virtualbox*'")],
            // The install's metapackages (qemu-system, libvirt-daemon-system) leave the packages
            // they pulled in behind, with the binaries the Machine page looks for
            // (qemu-system-x86_64, virsh): those go too, or the tool still reads as installed.
            Dependency::Qemu => vec![root("apt-get remove -y 'qemu-system*' qemu-utils")],
            Dependency::Libvirt => vec![root("apt-get remove -y 'libvirt-daemon*' libvirt-clients virt-manager")],
            Dependency::Awscli => vec![root("rm -rf /usr/local/aws-cli /usr/local/bin/aws /usr/local/bin/aws_completer")],
            Dependency::Azurecli => vec![root("apt-get remove -y azure-cli")],
            Dependency::Gcloud => vec![step("sh", &["-c", "rm -rf \"$HOME/google-cloud-sdk\""])],
            Dependency::Utm | Dependency::Wsl => return Err(Error::Invalid("Not installed on Linux.".into())),
        })
    }
}

/// Removes a tool Cyber CTF installed, streaming the output, then forgets it.
#[tauri::command]
pub async fn uninstall_dependency(app: AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    if !confirmed(load(&app)).iter().any(|t| t.dependency == dependency) {
        return Err(Error::Invalid("Cyber CTF didn't install this tool, so it won't remove it.".into()));
    }
    let mut on_line = channel_log(logs);
    run_steps(&plan(dependency)?, &mut on_line).await?;
    update(&app, |tools| tools.retain(|t| t.dependency != dependency));
    on_line("Removed.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_through_their_file_and_keep_one_entry_per_tool() {
        let dir = std::env::temp_dir().join(format!("cyberctf-tools-{}", rand::random::<u32>()));
        let f = dir.join("data").join("installed-tools.json");
        assert!(load_at(&f).is_empty());
        let mut tools = load_at(&f);
        remember(&mut tools, Dependency::Vagrant, false);
        remember(&mut tools, Dependency::Docker, false);
        remember(&mut tools, Dependency::Vagrant, false);
        save_at(&f, &tools);
        let back = load_at(&f);
        assert_eq!(back.iter().map(|t| t.dependency).collect::<Vec<_>>(), [Dependency::Docker, Dependency::Vagrant]);
        assert!(back.iter().all(|t| t.at > 0));
        assert!(!f.with_extension("json.tmp").exists());
        let json = serde_json::to_value(&back[0]).unwrap();
        assert_eq!(json["dependency"], "docker");
        std::fs::write(&f, "[{").unwrap();
        assert!(load_at(&f).is_empty());
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
        // Any mounted /Volumes/VirtualBox* was trusted, and its tool run as admin.
        assert!(!VIRTUALBOX_UNINSTALL.contains("/Volumes/VirtualBox"), "{VIRTUALBOX_UNINSTALL}");
        assert!(!VIRTUALBOX_UNINSTALL.contains("exec "));
        assert!(VIRTUALBOX_UNINSTALL.contains("Caches/Homebrew/downloads/*VirtualBox*.dmg"));
        assert!(VIRTUALBOX_UNINSTALL.contains("hdiutil detach"));
        assert!(crate::platform::sh_parses(VIRTUALBOX_UNINSTALL));
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
            assert_eq!(plan(dep).unwrap()[0].program, "winget", "{dep:?}");
        }
        assert!(plan(Dependency::Wsl).is_err() && plan(Dependency::Utm).is_err() && plan(Dependency::Libvirt).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_macos_tool_has_a_way_out() {
        if super::super::brew_bin().is_none() {
            return; // CI without Homebrew: the brew-based plans can't be built.
        }
        for dep in [
            Dependency::Docker,
            Dependency::Vagrant,
            Dependency::Terraform,
            Dependency::Virtualbox,
            Dependency::Qemu,
            Dependency::Utm,
            Dependency::Awscli,
            Dependency::Azurecli,
            Dependency::Gcloud,
        ] {
            assert!(!plan(dep).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl).is_err() && plan(Dependency::Libvirt).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_linux_tool_has_a_way_out() {
        for dep in [
            Dependency::Docker,
            Dependency::Vagrant,
            Dependency::Terraform,
            Dependency::Virtualbox,
            Dependency::Qemu,
            Dependency::Libvirt,
            Dependency::Awscli,
            Dependency::Azurecli,
            Dependency::Gcloud,
        ] {
            assert!(!plan(dep).unwrap().is_empty(), "{dep:?}");
        }
        assert!(plan(Dependency::Wsl).is_err() && plan(Dependency::Utm).is_err());
        // The packages holding the binaries the Machine page looks for go too, not only the
        // metapackages the install named.
        let script = |dep| plan(dep).unwrap()[0].args.join(" ");
        assert!(script(Dependency::Qemu).contains("'qemu-system*'"));
        assert!(script(Dependency::Libvirt).contains("libvirt-clients"));
    }
}
