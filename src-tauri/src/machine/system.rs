//! The system check: which tools, engines, hypervisors and package manager this machine has,
//! and what Isoloom says it can run. Also the machine's live metrics and its setup window.

pub mod docker;
pub mod metrics;
pub mod wsl;

use serde::Serialize;

use crate::platform::exec::{docker_denied, docker_denied_error, run_read};
use crate::runtime::providers::{self, ProviderStatus};

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub installed: bool,
    pub version: Option<String>,
}

/// The OS package manager our one-click installs rely on (brew / winget / apt).
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PkgManager {
    pub name: String,
    pub installed: bool,
    /// First line of its version output, e.g. "Homebrew 4.6.3", when installed.
    pub version: Option<String>,
}

/// Cloud provider CLIs, for the Cloud setup (AWS / Azure / GCP).
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CloudClis {
    pub aws: Tool,
    pub azure: Tool,
    pub gcloud: Tool,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub os: &'static str,
    pub arch: &'static str,
    pub pkg_manager: PkgManager,
    pub docker: Tool,
    /// The Docker daemon answers (Docker Desktop / engine is started).
    pub docker_running: bool,
    /// The daemon runs but refused this account (not in the `docker` group yet): starting the
    /// engine won't help; `docker_denied_hint` says what will (as an error with its code, for
    /// the webview to say in the player's language).
    pub docker_denied: bool,
    pub docker_denied_hint: Option<crate::error::Wire>,
    /// Which Docker-compatible engine answers, when one does: `docker-desktop`, `orbstack`,
    /// `colima`, `rancher-desktop`, `podman` or `docker-engine`.
    #[ts(type = r#""docker-desktop" | "orbstack" | "colima" | "rancher-desktop" | "podman" | "docker-engine" | null"#)]
    pub docker_engine: Option<&'static str>,
    /// Every engine whose Docker context answers right now. Several can run side by side
    /// (e.g. Docker Desktop and OrbStack); `docker_engine` is the one the CLI uses.
    #[ts(type = r#"("docker-desktop" | "orbstack" | "colima" | "rancher-desktop" | "podman" | "docker-engine")[]"#)]
    pub docker_engines_running: Vec<&'static str>,
    pub docker_compose: Tool,
    pub vagrant: Tool,
    /// A local terraform binary (preferred for provisioning state; container is the fallback).
    pub terraform: Tool,
    /// VMware's OVF Tool, which the ESXi Vagrant plugin uses to upload VMs (ships with
    /// VMware Fusion / Workstation, or standalone from Broadcom).
    pub ovftool: Tool,
    pub cloud_clis: CloudClis,
    /// Vagrant providers (local hypervisors and remote hosts such as ESXi).
    /// Whether a given VM lab can run also depends on its boxes existing for
    /// this architecture: x86-only boxes (e.g. pfSense) cannot run on ARM hosts
    /// with a local hypervisor, but can on a remote x86 host.
    pub vm_providers: Vec<ProviderStatus>,
    /// What Isoloom says this machine can run, target by target (`isoloom targets --host`):
    /// the tools and credentials each one needs, found or missing.
    pub targets: Vec<TargetReadiness>,
    /// Windows: whether WSL 2 is on, waits for a restart, or is missing (`None` elsewhere).
    pub wsl: Option<wsl::WslReport>,
}

/// One Isoloom target on this machine.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TargetReadiness {
    /// The target id: `docker`, `vagrant`, `proxmox`, `cloud-vm`, ...
    pub target: String,
    /// The cloud, for the cloud targets (`aws`, `azure`, ...).
    pub cloud: Option<String>,
    pub ready: bool,
    /// One line: what was found, or what is missing.
    pub summary: String,
    /// Each thing found (plain) or missing (starts with `!`).
    pub notes: Vec<String>,
}

pub async fn system_check(dirs: crate::platform::AppDirs) -> SystemReport {
    // A tool installed since the last check (from the app or not) is found without a restart.
    crate::platform::env_path::refresh();
    let (docker, docker_compose, vagrant, daemon, aws, azure, gcloud, terraform) = tokio::join!(
        probe("docker", &["--version"]),
        probe("docker", &["compose", "version", "--short"]),
        probe("vagrant", &["--version"]),
        docker::daemon_os(),
        probe("aws", &["--version"]),
        probe_azure(),
        probe("gcloud", &["--version"]),
        probe("terraform", &["version"]),
    );
    let (vm_providers, docker_engines_running, ovftool, targets, wsl) =
        tokio::join!(providers::detect(vagrant.installed), docker::running_engines(), probe("ovftool", &["--version"]), isoloom_targets(), wsl::report(&dirs));
    let docker_denied = daemon.as_ref().err().is_some_and(docker_denied);
    let docker_engine = match &daemon {
        Ok(os) => Some(docker::engine_in_use(os.trim()).await),
        Err(_) => None,
    };
    SystemReport {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        pkg_manager: package_manager().await,
        docker,
        docker_running: daemon.is_ok(),
        docker_denied,
        docker_denied_hint: docker_denied.then(|| docker_denied_error().to_wire()),
        docker_engine,
        docker_engines_running,
        docker_compose,
        vagrant,
        terraform,
        ovftool,
        cloud_clis: CloudClis { aws, azure, gcloud },
        vm_providers,
        targets,
        wsl,
    }
}

/// A tool's version, timed: a wedged daemon or a held Vagrant lock must not hang the check.
async fn probe(program: &'static str, args: &[&str]) -> Tool {
    match run_read(program, args, None).await {
        Ok(out) => Tool { installed: true, version: first_line(&out) },
        Err(_) => Tool { installed: false, version: None },
    }
}

/// `az version` prints JSON (`{"azure-cli": "2.66.0", ...}`); read the version from it here. A
/// `--query "\"azure-cli\""` would need quotes, which can't pass through cmd.exe (`az` is a .cmd).
async fn probe_azure() -> Tool {
    match run_read("az", &["version", "--output", "json"], None).await {
        Ok(out) => Tool { installed: true, version: azure_version(&out) },
        Err(_) => Tool { installed: false, version: None },
    }
}

fn azure_version(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    value.get("azure-cli")?.as_str().map(str::to_owned)
}

/// The first non-blank line, trimmed: a tool whose output opens with a blank line (a banner's
/// spacing) read as installed with an empty version.
fn first_line(out: &str) -> Option<String> {
    out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

async fn package_manager() -> PkgManager {
    pkg_manager(package_manager_version().await)
}

/// This OS's package manager, given its `--version` output when it is installed.
fn pkg_manager(version: Option<String>) -> PkgManager {
    let name = if cfg!(target_os = "windows") {
        "winget"
    } else if cfg!(target_os = "linux") {
        "apt"
    } else {
        "Homebrew"
    };
    let installed = version.is_some();
    let version = version.and_then(|v| first_line(&v)).filter(|v| !v.is_empty());
    PkgManager { name: name.to_string(), installed, version }
}

/// `--version` of the package manager, when it is installed.
async fn package_manager_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let brew = crate::platform::brew_bin()?;
        Some(tokio::process::Command::new(brew).arg("--version").output().await.ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default())
    }
    #[cfg(target_os = "windows")]
    {
        crate::platform::exec::run("winget", &["--version"], None).await.ok()
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::exec::run("apt-get", &["--version"], None).await.ok()
    }
}

/// Isoloom's own view of the host, off the async runtime (it runs the tools).
async fn isoloom_targets() -> Vec<TargetReadiness> {
    tokio::task::spawn_blocking(|| {
        crate::runtime::lab::host_readiness()
            .into_iter()
            .map(|r| TargetReadiness {
                target: r.target.id().to_string(),
                cloud: r.cloud.clone(),
                ready: r.ready,
                summary: r.summary(),
                notes: r.notes.clone(),
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    /// Runs the real probes against this machine: `cargo test -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "depends on the host's installed tools"]
    async fn prints_this_machines_report() {
        let t = crate::testing::test_app::TestApp::new();
        let report = super::system_check(t.dirs()).await;
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }

    #[test]
    fn the_azure_cli_version_comes_from_its_json_without_a_quoted_query() {
        let out = "{\n  \"azure-cli\": \"2.90.0\",\n  \"azure-cli-core\": \"2.90.0\",\n  \"extensions\": {}\n}\n";
        assert_eq!(super::azure_version(out).as_deref(), Some("2.90.0"));
        assert_eq!(super::azure_version("not json"), None);
        assert_eq!(super::azure_version("{}"), None);
        // The probe's arguments must pass the cmd.exe guard, or `az` reads as missing everywhere.
        assert!(crate::platform::exec::shim::check_args("az", &["version", "--output", "json"]).is_ok());
    }

    #[test]
    fn a_version_is_the_first_line_trimmed() {
        assert_eq!(super::first_line("Homebrew 4.6.3 \nHomebrew/core\n").as_deref(), Some("Homebrew 4.6.3"));
        assert_eq!(super::first_line(""), None);
        assert_eq!(super::first_line("\n  \nVagrant 2.4.9\n").as_deref(), Some("Vagrant 2.4.9"));
    }

    #[test]
    fn a_version_on_windows_line_endings_or_without_a_newline_has_no_stray_characters() {
        // A tool on Windows ends its lines with CRLF; one printing a single line ends it with
        // nothing at all. Neither leaves a `\r` or needs a `\n`.
        assert_eq!(super::first_line("Vagrant 2.4.3\r\n\r\n").as_deref(), Some("Vagrant 2.4.3"));
        assert_eq!(super::first_line("Terraform v1.9.8").as_deref(), Some("Terraform v1.9.8"));
        assert_eq!(super::first_line("\tDocker version 27.3.1, build x\t\r\n").as_deref(), Some("Docker version 27.3.1, build x"));
        // Only blank lines: no version at all.
        assert_eq!(super::first_line("\r\n"), None);
        assert!(super::pkg_manager(Some("\r\n".into())).version.is_none());
        assert_eq!(super::azure_version("{\"azure-cli\": \"2.90.0\"}\r\n").as_deref(), Some("2.90.0"));
        assert_eq!(super::azure_version("{\"azure-cli\": 2.9}"), None, "a version that isn't text");
    }

    #[test]
    fn azure_version_comes_from_az_version_json() {
        let out = "{\r\n  \"azure-cli\": \"2.66.0\",\r\n  \"azure-cli-core\": \"2.66.0\",\r\n  \"extensions\": {}\r\n}\r\n";
        assert_eq!(super::azure_version(out).as_deref(), Some("2.66.0"));
        assert_eq!(super::azure_version("WARNING: something\n{}"), None);
        assert_eq!(super::azure_version("{\"azure-cli\": 2}"), None);
        assert_eq!(super::azure_version(""), None);
    }

    #[test]
    fn the_package_manager_reports_its_first_version_line() {
        let pm = super::pkg_manager(Some("Homebrew 4.6.3\nHomebrew/core\n".into()));
        assert!(pm.installed);
        assert_eq!(pm.version.as_deref(), Some("Homebrew 4.6.3"));
        assert_ne!(pm.name, "");
        let empty = super::pkg_manager(Some(String::new()));
        assert!(empty.installed && empty.version.is_none());
        let missing = super::pkg_manager(None);
        assert!(!missing.installed && missing.version.is_none());
        let v = serde_json::to_value(super::Tool { installed: true, version: Some("1".into()) }).unwrap();
        assert_eq!(v, serde_json::json!({ "installed": true, "version": "1" }));
    }
}

#[cfg(test)]
mod proptests;
