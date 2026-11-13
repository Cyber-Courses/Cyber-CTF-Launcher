//! The system check: which tools, engines, hypervisors and package manager this machine has,
//! and what Isoloom says it can run. Also the machine's live metrics and its setup window.

pub mod docker;
pub mod metrics;
pub mod wsl;

use serde::Serialize;

use crate::platform::exec::{docker_denied, docker_denied_error, run_read};
use crate::platform::remembered::remembered;
use crate::runtime::providers::{self, ProviderStatus};

/// A command-line tool the machine check probes.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Whether it answered its version probe.
    pub installed: bool,
    /// The first line of its version output, when it answered.
    pub version: Option<String>,
}

/// The OS package manager our one-click installs rely on (brew / winget / apt).
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PkgManager {
    /// Its name: `Homebrew`, `winget`, `apt` or `dnf`.
    pub name: String,
    /// Whether it answered its version probe.
    pub installed: bool,
    /// First line of its version output, e.g. "Homebrew 4.6.3", when installed.
    pub version: Option<String>,
}

/// Cloud provider CLIs, for the Cloud setup (AWS / Azure / GCP).
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CloudClis {
    /// The AWS CLI (`aws`).
    pub aws: Tool,
    /// The Azure CLI (`az`).
    pub azure: Tool,
    /// The Google Cloud CLI (`gcloud`).
    pub gcloud: Tool,
}

/// What the machine check found on this computer: the OS, the tools, the container engines and
/// hypervisors, and what the guided setup takes from here.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    /// The OS, as Rust names it (`macos`, `windows`, `linux`).
    pub os: &'static str,
    /// The CPU architecture, as Rust names it (`aarch64`, `x86_64`).
    pub arch: &'static str,
    /// The OS's package manager.
    pub pkg_manager: PkgManager,
    /// The Docker CLI.
    pub docker: Tool,
    /// The Docker daemon answers (Docker Desktop / engine is started).
    pub docker_running: bool,
    /// The daemon runs but refused this account (not in the `docker` group yet): starting the
    /// engine won't help; `docker_denied_hint` says what will (as an error with its code, for
    /// the webview to say in the player's language).
    pub docker_denied: bool,
    /// Why the daemon refused this account and what to do, as a coded error, when it did.
    pub docker_denied_hint: Option<crate::error::Wire>,
    /// Which Docker-compatible engine answers, when one does.
    pub docker_engine: Option<docker::Engine>,
    /// Every engine whose Docker context answers right now. Several can run side by side
    /// (e.g. Docker Desktop and OrbStack); `docker_engine` is the one the CLI uses.
    pub docker_engines_running: Vec<docker::Engine>,
    /// Every Docker-compatible engine found on this machine (one per CLI context that names
    /// one or answers), each with its own state: the Machine page lists them all, not only
    /// the one the CLI uses.
    pub docker_engines: Vec<docker::EngineStatus>,
    /// Docker Compose (`docker compose`).
    pub docker_compose: Tool,
    /// Vagrant.
    pub vagrant: Tool,
    /// A local terraform binary (preferred for provisioning state; container is the fallback).
    pub terraform: Tool,
    /// VMware's OVF Tool, which the ESXi Vagrant plugin uses to upload VMs (ships with
    /// VMware Fusion / Workstation, or standalone from Broadcom).
    pub ovftool: Tool,
    /// Where the OVF Tool was found (Fusion's or Workstation's copy, the app's, or `PATH`), when
    /// one was, even if it doesn't run.
    pub ovftool_location: Option<crate::machine::tools::ovftool::Location>,
    /// The cloud providers' CLIs.
    pub cloud_clis: CloudClis,
    /// Vagrant providers (local hypervisors and remote hosts such as ESXi).
    /// Whether a given VM lab can run also depends on its boxes existing for
    /// this architecture: x86-only boxes (e.g. pfSense) cannot run on ARM hosts
    /// with a local hypervisor, but can on a remote x86 host.
    pub vm_providers: Vec<ProviderStatus>,
    /// The local hypervisors of `vm_providers` ready for a lab's VMs ([`providers::ready`]).
    pub ready_hypervisors: Vec<providers::Provider>,
    /// Windows: whether WSL 2 is on, waits for a restart, or is missing (`None` elsewhere).
    pub wsl: Option<wsl::WslReport>,
    /// The groups this account joined since it logged in (Linux: the docker, libvirt and kvm
    /// groups the installs add it to), which apply from its next login.
    pub groups_pending: Vec<String>,
    /// What each choice of the guided setup takes on this machine ([`crate::machine::setup`]):
    /// the setup window renders these, never working them out itself.
    pub setup: crate::machine::setup::Options,
}

/// Checks this machine: the tools, the container engines, the hypervisors, WSL and the groups
/// pending a login, and what the guided setup takes from there.
pub async fn system_check(dirs: crate::platform::AppDirs) -> SystemReport {
    let data = dirs.data().ok().map(std::path::Path::to_path_buf);
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
    let (vm_providers, docker_engines, (ovftool, ovftool_location), wsl, groups_pending) = tokio::join!(
        providers::detect(vagrant.installed),
        docker::engines(),
        crate::machine::tools::ovftool::probe(data.as_deref()),
        wsl::report(&dirs),
        crate::platform::exec::groups_pending()
    );
    let docker_engines_running = docker::running_of(&docker_engines);
    let docker_denied = daemon.as_ref().err().is_some_and(docker_denied);
    let docker_engine = match &daemon {
        Ok(os) => Some(docker::engine_in_use(os.trim()).await),
        Err(_) => None,
    };
    let ready_hypervisors = providers::ready(&vm_providers, vagrant.installed);
    let mut report = SystemReport {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        pkg_manager: package_manager().await,
        docker,
        docker_running: daemon.is_ok(),
        docker_denied,
        docker_denied_hint: docker_denied.then(|| docker_denied_error().to_wire()),
        docker_engine,
        docker_engines_running,
        docker_engines,
        docker_compose,
        vagrant,
        terraform,
        ovftool,
        ovftool_location,
        cloud_clis: CloudClis { aws, azure, gcloud },
        ready_hypervisors,
        vm_providers,
        wsl,
        groups_pending,
        setup: crate::machine::setup::Options::empty(),
    };
    // Hyper-V turned on from here starts at the next restart: until then the setup asks for it.
    let hyperv_restart = cfg!(windows) && wsl::hyperv_on_this_boot(&dirs);
    report.setup = crate::machine::setup::options(&report, hyperv_restart);
    report
}

/// A tool's version, timed: a wedged daemon or a held Vagrant lock must not hang the check. A
/// `--version` answer is kept a while (`platform::remembered`): it changes only with an install.
async fn probe(program: &'static str, args: &[&str]) -> Tool {
    let answer = if args == ["--version"] {
        // Boxed: kept in the machine check's future, it would double every probe's size.
        Box::pin(remembered(&format!("{program} --version"), run_read(program, args, None))).await
    } else {
        run_read(program, args, None).await
    };
    match answer {
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
pub(crate) fn first_line(out: &str) -> Option<String> {
    out.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

async fn package_manager() -> PkgManager {
    pkg_manager(package_manager_version().await)
}

/// This OS's package manager, given its `--version` output when it is installed: on Linux apt,
/// or dnf where there is no apt-get (Fedora and the distributions built on it).
fn pkg_manager(version: Option<String>) -> PkgManager {
    let name = if cfg!(target_os = "windows") {
        "winget"
    } else if cfg!(target_os = "linux") {
        linux_packages_name()
    } else {
        "Homebrew"
    };
    let installed = version.is_some();
    let version = version.and_then(|v| first_line(&v)).filter(|v| !v.is_empty());
    PkgManager { name: name.to_string(), installed, version }
}

/// `--version` of the package manager, when it is installed: Homebrew (where it is installed),
/// winget or apt.
async fn package_manager_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    let program = crate::platform::brew_bin()?;
    #[cfg(target_os = "windows")]
    let program = "winget";
    #[cfg(target_os = "linux")]
    let program = if linux_packages_name() == "dnf" { "dnf" } else { "apt-get" };
    version_of(program).await
}

/// The Linux package manager's name: `apt` where apt-get is on PATH, `dnf` where dnf is and
/// apt-get isn't, else `apt` (the name the probe then reports as not installed).
fn linux_packages_name() -> &'static str {
    if !crate::platform::on_path("apt-get") && crate::platform::on_path("dnf") { "dnf" } else { "apt" }
}

/// `program --version`, timed as every probe is ([`run_read`]); none when it can't run or fails
/// (a failed `brew --version` read as installed, with no version).
async fn version_of(program: &'static str) -> Option<String> {
    run_read(program, &["--version"], None).await.ok()
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

    #[tokio::test]
    async fn a_package_manager_that_fails_its_version_is_not_installed() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let _broken = Tools::new(|program, _| if program == "brew" { fail("Error: brew is broken") } else { None });
        assert!(!super::pkg_manager(super::version_of("brew").await).installed);
        let _working = Tools::new(|program, _| if program == "brew" { ok("Homebrew 4.6.3\n") } else { None });
        assert_eq!(super::pkg_manager(super::version_of("brew").await).version.as_deref(), Some("Homebrew 4.6.3"));
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

    /// The check reports the OVF Tool the setup placed in the app's folder, and where it is.
    #[tokio::test]
    async fn the_check_says_where_the_ovf_tool_was_found() {
        use crate::machine::tools::ovftool::{Source, managed_dir};
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let _tools = Tools::new(|program, _| if program == "ovftool" { ok("VMware ovftool 5.1.0 (build-24936374)\n") } else { None });
        let report = super::system_check(t.dirs()).await;
        let fusion = std::path::Path::new("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool").is_file();
        if report.ovftool_location.is_none() {
            assert!(!report.ovftool.installed, "nothing found, nothing installed");
        }
        let exe = if cfg!(windows) { "ovftool.exe" } else { "ovftool" };
        std::fs::create_dir_all(managed_dir(t.data())).unwrap();
        std::fs::write(managed_dir(t.data()).join(exe), "").unwrap();
        let report = super::system_check(t.dirs()).await;
        assert_eq!(report.ovftool.version.as_deref(), Some("VMware ovftool 5.1.0 (build-24936374)"));
        let location = report.ovftool_location.unwrap();
        if !fusion && !cfg!(windows) {
            assert_eq!((location.source, std::path::PathBuf::from(&location.path)), (Source::Managed, managed_dir(t.data()).join(exe)));
        }
        let json = serde_json::to_value(&location).unwrap();
        assert!(json.get("needsRosetta").is_some() && json.get("source").is_some(), "{json}");
    }

    /// A tool whose probe fails (a wedged daemon, a tool refusing `--version`; a timed-out probe
    /// is the same failure) or that isn't installed reads as not installed with no version, and
    /// the check runs on: a report is always produced, never an error for one tool.
    #[tokio::test]
    async fn a_probe_that_fails_or_finds_no_tool_reads_as_not_installed() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let _tools = Tools::new(|program, _| match program {
            "docker" => fail("Cannot connect to the Docker daemon at unix:///var/run/docker.sock"),
            "vagrant" | "az" => None,
            _ => ok("Terraform v1.9.8\n"),
        });
        let failing = super::probe("docker", &["--version"]).await;
        assert!(!failing.installed && failing.version.is_none());
        let missing = super::probe("vagrant", &["--version"]).await;
        assert!(!missing.installed && missing.version.is_none());
        let found = super::probe("terraform", &["version"]).await;
        assert_eq!((found.installed, found.version.as_deref()), (true, Some("Terraform v1.9.8")));
        let az = super::probe_azure().await;
        assert!(!az.installed && az.version.is_none());
        // The whole check, with Docker down and Vagrant missing: the report says so for them.
        let t = crate::testing::test_app::TestApp::new();
        let report = super::system_check(t.dirs()).await;
        assert!(!report.docker.installed && !report.docker_running && !report.docker_denied);
        assert!(!report.vagrant.installed && !report.cloud_clis.azure.installed);
        assert!(report.terraform.installed);
    }
}

#[cfg(test)]
mod proptests;
