//! Where the lab the box joins runs, and how the box goes in and out of it there: the box added
//! to (or taken out of) the lab's spec, the lab regenerated for its form, then the run brought up
//! to date with the regenerated lab, with what changing it needs (a cloud account's credentials,
//! a server's connection). The same on every form; only the run and its place differ.

use std::path::{Path, PathBuf};

use crate::runtime::lab::Target;

use super::status::{AttackStatus, status_of};
use super::{Cpu, Kind};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::host::{Arch, Os};
use crate::runtime::host::{self, Failed, Host};
use crate::runtime::providers::Provider;
use crate::runtime::{Runtime, lab};

/// The lab the box joins: its run (`host`, [`crate::runtime::host`]), its folder, the Isoloom
/// target it is generated for, and whether its machines are VMs (the box is Kali then) or
/// containers (the picked image).
pub(crate) struct Placed {
    /// Where the lab runs, and its run there.
    pub(crate) host: Host,
    /// The lab's folder.
    pub(crate) dir: PathBuf,
    /// The Isoloom target the lab is generated for.
    pub(crate) target: Target,
    /// Whether the lab's machines are VMs (the box is a Kali VM) rather than containers.
    pub(crate) vm: bool,
}

/// Where lab `id`, installed in `dir` and declared `runtime`, runs, as [`host::locate`] finds it:
/// its run on this machine, on a Proxmox server or in a cloud account. `None` where the launcher
/// adds no box: on an ESXi host, not supported yet (a choice, not a limit of the run: the runner
/// updates an ESXi run like any other), beside cloud services, which have no machines for it
/// to join ([`Host::has_machines`]), wherever they run, or in a helper VM an earlier version ran
/// a container lab inside, whose files can't be generated again with the box.
pub(crate) async fn locate(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<Option<Placed>> {
    Ok(match host::locate(dirs, dir, id, runtime).await? {
        None | Some(Host::Esxi { .. }) => None,
        Some(host) if !host.has_machines() => None,
        // Its machines are VMs wherever its target runs them (on Proxmox, whatever its catalogue form).
        Some(host) => host.target().map(|target| Placed { target, host, dir: dir.to_path_buf(), vm: !matches!(target, Target::Docker | Target::CloudDocker) }),
    })
}

/// Where lab `id`'s attack box goes ([`locate`]), or the error an operation on the box gets when
/// the launcher reaches no run of the lab it could join.
pub(crate) async fn reachable(dirs: &AppDirs, dir: &Path, id: &str, runtime: Runtime) -> Result<Placed> {
    locate(dirs, dir, id, runtime)
        .await?
        .ok_or_else(|| Error::Invalid("This lab has no attack box the launcher can reach. Work from what the lab gives you.".into()))
}

/// What the learner picked for the box: the image a container box runs, and whether a VM box gets
/// Kali's desktop.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pick<'a> {
    /// The image a container box runs.
    pub(crate) image: &'a str,
    /// A VM box gets Kali's desktop.
    pub(crate) desktop: bool,
}

impl Placed {
    /// Adds the box onto `network` (else the lab's first machine's), from `image` beside
    /// containers, then brings the run up to date with it.
    ///
    /// A failure before the run could have changed puts the lab back as it was (the box out of
    /// its spec, its files regenerated): the box's spec or the lab's files couldn't be written,
    /// the run's place couldn't be reached, or the runner refused before acting (the run isn't
    /// up, another operation holds it). A failure while the runner changed the run keeps the box
    /// in the lab, so the learner's Stop retires whatever of it the run already has.
    pub(crate) async fn add(&self, dirs: &AppDirs, pick: Pick<'_>, network: Option<&str>, log: &mut impl FnMut(String)) -> Result<()> {
        let kind = if self.vm { Kind::Vm { desktop: pick.desktop } } else { Kind::Container { image: pick.image } };
        let shared = self.shared_folder(dirs, log).await?;
        let written = super::add(&self.dir, kind, self.cpu(), network, shared.as_deref()).and_then(|()| lab::prepare(&self.dir, self.target).map(drop));
        let failed = match written {
            Err(error) => Failed { error, run_changed: false },
            Ok(()) => match self.host.update(dirs, log).await {
                Ok(()) => return Ok(()),
                Err(failed) => failed,
            },
        };
        if !failed.run_changed
            && let Err(e) = self.undo_add()
        {
            log::warn!("couldn't put the lab back as it was after a failed attack box add: {e}");
        }
        Err(failed.error)
    }

    /// The lab as it was before an add: the box out of its spec, its files regenerated.
    fn undo_add(&self) -> Result<()> {
        super::remove(&self.dir)?;
        lab::prepare(&self.dir, self.target).map(drop)
    }

    /// Takes the box out: retired from the run (what the update wouldn't remove itself), out of
    /// the lab, then the run brought up to date. Nothing to do when the lab hasn't it.
    pub(crate) async fn remove(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<()> {
        if !super::added(&self.dir)? {
            return Ok(());
        }
        self.host.retire(dirs, &super::name(&self.dir)?, log).await?;
        super::remove(&self.dir)?;
        lab::prepare(&self.dir, self.target)?;
        self.host.update(dirs, log).await.map_err(|f| f.error)
    }

    /// The box as the lab page shows it: read by its name from the run's status (down before
    /// the lab's files exist), with the networks it can join ([`super::networks`]).
    pub(crate) async fn status(&self) -> Result<AttackStatus> {
        let super::Networks { pickable, default } = super::networks(&self.dir)?;
        let seen = match self.host.run_status().await? {
            None => AttackStatus { vm: self.vm, ..AttackStatus::default() },
            Some(status) => {
                let name = super::name(&self.dir)?;
                // The network the box joined, as the lab's spec (with the box added) says.
                let spec = lab::lab(&self.dir)?.spec();
                let network = spec.machines.get(&name).and_then(|m| m.networks.keys().next().cloned());
                let declared = network.as_deref().and_then(|net| declared_address(&self.dir, &name, net, self.vm));
                status_of(&status, &name, network.as_deref(), declared.as_deref(), self.vm)
            }
        };
        Ok(AttackStatus { reachable: true, networks: pickable, default_network: default, ..seen })
    }

    /// The command that opens a shell into the box, as the player's account ([`super::PLAYER`]).
    pub(crate) async fn shell_command(&self) -> Result<isothrum_runner::ShellCommand> {
        self.host.console(&super::name(&self.dir)?, Some(super::PLAYER)).await
    }

    /// The learner's shared folder for the box, when it is on and the lab runs where a machine
    /// can mount a folder of this computer ([`shares_folders`]); otherwise none, `log` saying why
    /// when it is on (or when the setting or the folder can't be used). Refused when the
    /// hypervisor lacks what it shares the folder through (libvirt's or QEMU's virtiofsd on Linux,
    /// [`crate::runtime::providers::can_share`]): said before the box is added, not as the
    /// hypervisor's failure once its VM starts.
    async fn shared_folder(&self, dirs: &AppDirs, log: &mut impl FnMut(String)) -> Result<Option<PathBuf>> {
        let folder = match crate::shared_folder::current(dirs) {
            Ok(Some(folder)) => folder,
            Ok(None) => return Ok(None),
            Err(e) => {
                log(format!("The shared folder isn't mounted in the attack box: {e}."));
                return Ok(None);
            }
        };
        // The run's record says its hypervisor: no status read (a `vagrant status`) for it.
        let provider = match &self.host {
            Host::Local(run) => match run.recorded_provider().await {
                Ok(provider) => provider,
                Err(e) => {
                    log::warn!("couldn't read the lab's hypervisor for the shared folder: {e}");
                    None
                }
            },
            Host::Esxi { .. } | Host::Cloud { .. } | Host::Server { .. } => None,
        };
        if shares_folders(self.local(), self.target, provider.as_deref(), Os::HERE) {
            if let Some(p) = provider.as_deref().and_then(Provider::from_id) {
                crate::runtime::providers::can_share(p)?;
            }
            return Ok(Some(folder));
        }
        let why = match provider.as_deref() {
            _ if !self.local() => "a lab on a server or in a cloud account can't reach a folder of this computer".to_owned(),
            Some(p) => format!("{p} can't share a folder of this computer with a VM"),
            None => "the lab's hypervisor isn't known, so sharing a folder with its VM isn't assumed".to_owned(),
        };
        log(format!("The shared folder isn't mounted in the attack box here: {why}."));
        Ok(None)
    }

    /// The box's CPU on this computer ([`box_cpu`]).
    fn cpu(&self) -> Cpu {
        box_cpu(Arch::HERE, self.local(), self.target)
    }

    /// Whether the box runs here, on this machine.
    fn local(&self) -> bool {
        matches!(self.host, Host::Local(_))
    }

    /// Whether adding the box pulls its image on this machine: a container box here. A Kali VM
    /// box pulls none, and an image pulled in a lab's VM on a server or in a cloud account isn't
    /// this machine's download to account for.
    pub(crate) fn pulls_image_here(&self) -> bool {
        !self.vm && self.local()
    }
}

/// Whether a machine of a lab of `target` can mount a folder of this computer (`os`, as Rust
/// names it), the lab run here (`local`) on `provider` (its hypervisor, for a VM form): Docker
/// binds it; a local VM shares it with its hypervisor's two-way folder type, which QEMU only has
/// on Linux (virtiofsd) and Hyper-V doesn't have without a password at every start (Isoloom's
/// `host_folders`); a lab on a server or in a cloud account can't reach this computer. The
/// integration tests' `shared_for` (`tests/integration/run.sh`) lists the same hypervisors, as
/// a test here checks.
fn shares_folders(local: bool, target: Target, provider: Option<&str>, os: Os) -> bool {
    match target {
        _ if !local => false,
        Target::Docker => true,
        Target::Vagrant => provider.and_then(Provider::from_id).is_some_and(|p| p.shares_folders_on(os)),
        _ => false,
    }
}

/// The box's CPU for a lab of `target`, run on this computer (`local`) or not, `arch` being this
/// computer's: this computer's own where the lab runs on it and it is
/// Arm (a lab's machines are amd64 unless they say otherwise), whatever the OS; the lab's on a server or in a cloud account, and
/// on an x86 computer.
fn box_cpu(arch: Arch, local: bool, target: Target) -> Cpu {
    let native = matches!(target, Target::Docker | Target::Vagrant);
    if local && native && arch == Arch::Aarch64 { Cpu::Arm } else { Cpu::Lab }
}

/// The address the lab's spec gives machine `name` on `network` in the form it runs in (its
/// Docker address in a container, its own as a VM); none when the spec can't be read.
fn declared_address(dir: &Path, name: &str, network: &str, vm: bool) -> Option<String> {
    let snapshot = lab::resolved(dir).ok()?;
    let key = if vm { "addresses" } else { "docker_addresses" };
    snapshot["machines"][name][key][network].as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use isothrum_runner::fake::{FakeProcesses, Reply};

    use super::*;
    use crate::runtime::isothrum;

    /// Every file of the lab's generated module for `cloud` (a container lab, `cloud-docker`), by
    /// its path under it.
    fn module(dir: &Path, cloud: &str) -> Vec<(PathBuf, String)> {
        let root = lab::terraform(dir, Runtime::Docker, cloud).0;
        let mut files = Vec::new();
        let mut todo = vec![root.clone()];
        while let Some(d) = todo.pop() {
            for entry in std::fs::read_dir(&d).unwrap().map(std::result::Result::unwrap) {
                let path = entry.path();
                if path.is_dir() {
                    todo.push(path);
                } else {
                    files.push((path.strip_prefix(&root).unwrap().to_path_buf(), std::fs::read_to_string(&path).unwrap_or_default()));
                }
            }
        }
        files.sort();
        files
    }

    /// A Terraform whose sign-in to AWS runs out once `ran_out` is set: its apply then refuses, as
    /// Terraform's AWS provider says it.
    fn signing_out(ran_out: Arc<AtomicBool>) -> FakeProcesses {
        FakeProcesses::model(move |call| match call.args.first().map(String::as_str) {
            // The runner's key, which the lab's instances let in.
            _ if call.program.ends_with("ssh-keygen") => {
                if let Some(key) = call.args.iter().skip_while(|a| *a != "-f").nth(1) {
                    std::fs::write(key, "private").unwrap();
                    std::fs::write(format!("{key}.pub"), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest runner").unwrap();
                }
                Reply::ok("")
            }
            Some("version") => Reply::ok(r#"{"terraform_version":"1.10.5"}"#),
            Some("show") => Reply::ok(
                r#"{"values":{"root_module":{"resources":[{"address":"aws_instance.web","mode":"managed","type":"aws_instance","values":{"id":"i-1"}}]}}}"#,
            ),
            Some("output") => Reply::ok("{}"),
            Some("apply") if ran_out.load(Ordering::SeqCst) => Reply::fail(
                1,
                "Error: failed to refresh cached credentials: login session has expired, please reauthenticate\n  with provider[\"registry.terraform.io/hashicorp/aws\"]",
            ),
            Some("apply") => isothrum::scripted_apply(call),
            _ => Reply::ok(""),
        })
    }

    /// An add in a cloud account whose sign-in ran out is refused before the run could change (the
    /// runner's report says it didn't): the box out of the lab's spec again, its files as they were.
    #[tokio::test]
    async fn an_attack_box_refused_for_an_expired_cloud_sign_in_leaves_the_lab_as_published() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10 } }\n",
        );
        let target = lab::terraform(&dir, Runtime::Docker, "aws").1;
        lab::prepare(&dir, target).unwrap();
        let home = dir.join("home");
        // Terraform, whose sign-in to AWS runs out once the lab is up.
        let expired = Arc::new(AtomicBool::new(false));
        let fake = Arc::new(signing_out(Arc::clone(&expired)));
        let mut config = isothrum_runner::RunnerConfig::new(crate::runtime::paths::runner_home_in(&home));
        config.processes = Arc::clone(&fake) as Arc<dyn isothrum_runner::ProcessRunner>;
        let run = isothrum::CloudRun::new_with(config, &dir, "t", Runtime::Docker, "aws").unwrap();
        let place =
            || isothrum::CloudPlace { region: Some("eu-west-3".into()), allowed_cidr: Some("203.0.113.7/32".into()), ..isothrum::CloudPlace::default() };
        run.up(&place(), &[], &mut |_| {}).await.unwrap();
        expired.store(true, Ordering::SeqCst);
        let before = module(&dir, "aws");
        let conn = crate::runtime::server::Connection {
            id: String::new(),
            provider: crate::runtime::providers::Provider::Aws,
            name: "aws".into(),
            env: Vec::new(),
            tf_vars: Vec::new(),
            tf_env: Vec::new(),
            region: None,
            auto_stop_hours: None,
        };
        let placed = Placed { host: Host::Cloud { run, place: place(), conn }, dir: dir.to_path_buf(), target, vm: false };
        let dirs = AppDirs::new(Some(home), None);
        let err = placed.add(&dirs, Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.unwrap_err();
        assert!(err.to_string().starts_with("Your sign-in to AWS has expired"), "{err}");
        assert!(!super::super::added(&dir).unwrap(), "the box is out of the lab's spec again");
        assert!(!dir.join(lab::overlay::FILE).exists(), "and the launcher's layer gone");
        assert_eq!(module(&dir, "aws"), before, "the lab's files as they were");
        // The runner's store (`state.db` in its home) and the app's (read for the shared folder)
        // are open files in the lab's folder, which Windows won't remove while they are.
        drop(placed);
        crate::store::forget(dirs.data().unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The shell the lab page opens into the box in lab `dir`, generated for `target`, run on
    /// this computer by `run`.
    async fn box_shell(dir: &Path, target: Target, run: Result<isothrum::LabRun>, vm: bool) -> isothrum_runner::ShellCommand {
        lab::prepare(dir, target).unwrap();
        let placed = Placed { host: Host::Local(run.unwrap()), dir: dir.to_path_buf(), target, vm };
        placed.shell_command().await.unwrap()
    }

    #[tokio::test]
    async fn the_box_shell_opens_as_the_player_account_on_every_local_form() {
        // Regression: it opened as the account the way in logs in as: root in a container,
        // `vagrant` in a VM (on Windows' Hyper-V as elsewhere).
        use super::super::{NAME, PLAYER};
        let lab = |machine: &str| {
            crate::runtime::test_support::write_lab(&format!(
                "version: 1\nname: t\nnetworks: {{ lan: {{ cidr: 10.70.0.0/24 }} }}\nmachines:\n  web: {{ {machine}, networks: {{ lan: 10 }} }}\n"
            ))
        };
        let dir = lab("app: { image: nginx }");
        super::super::add(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, None).unwrap();
        let home = dir.join("home");
        let shell = box_shell(&dir, Target::Docker, isothrum::LabRun::docker(&home, &dir, "t"), false).await;
        let line = shell.args.join(" ");
        assert!(line.contains(&format!(" exec --user {PLAYER} {NAME} sh -c ")), "a container: exec as the account: {line}");
        let compose = std::fs::read_dir(&shell.cwd).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "yml"));
        let compose: String = compose.map(|p| std::fs::read_to_string(p).unwrap()).collect();
        assert!(compose.contains(&format!("account={PLAYER} shared=/shared password_hash=")), "the box makes its account where it starts: {compose}");

        let dir = lab("os: debian@12");
        super::super::add(&dir, Kind::Vm { desktop: false }, Cpu::Lab, None, None).unwrap();
        let home = dir.join("home");
        let shell = box_shell(&dir, Target::Vagrant, isothrum::LabRun::vm(&home, &dir, "t"), true).await;
        assert_eq!(shell.args[..3], ["ssh", NAME, "-c"], "{:?}", shell.args);
        assert!(shell.args[3].contains(&format!("exec sudo -iu {PLAYER}")), "a VM: a sudo login as the account: {:?}", shell.args);
    }

    #[test]
    fn the_shared_folder_reaches_the_box_wherever_the_lab_runs_on_this_computer_and_can_share_it() {
        assert!(shares_folders(true, Target::Docker, None, Os::Macos), "Docker binds it");
        for p in ["virtualbox", "vmware_desktop", "parallels", "utm"] {
            assert!(shares_folders(true, Target::Vagrant, Some(p), Os::Macos), "{p}");
        }
        assert!(shares_folders(true, Target::Vagrant, Some("qemu"), Os::Linux), "QEMU with virtiofsd");
        assert!(!shares_folders(true, Target::Vagrant, Some("qemu"), Os::Macos), "no virtiofsd on macOS");
        assert!(!shares_folders(true, Target::Vagrant, Some("hyperv"), Os::Windows), "SMB asks for a password");
        assert!(!shares_folders(true, Target::Vagrant, None, Os::Linux), "an unknown hypervisor: not assumed");
        for t in [Target::CloudDocker, Target::CloudVm, Target::Proxmox, Target::Vagrant] {
            assert!(!shares_folders(false, t, Some("virtualbox"), Os::Linux), "{t:?} away from this computer");
        }
    }

    /// Whether the integration tests' `shared_for` (`tests/integration/run.sh`) gives a VM
    /// box on `provider` a shared folder on `os`, read from its `case` arms: a plain `echo`
    /// always, one behind `uname -s` = Linux only there, any other arm none.
    fn script_shares(script: &str, provider: &str, os: Os) -> bool {
        let body = script.split("shared_for() {").nth(1).and_then(|b| b.split("esac").next()).expect("shared_for's case");
        let arm = body.lines().filter_map(|l| l.split_once(')')).find(|(pattern, _)| pattern.split('|').any(|p| p.trim() == provider));
        match arm {
            Some((_, action)) if action.contains("uname -s") => action.contains("= Linux") && os == Os::Linux,
            Some((_, action)) => action.contains("echo"),
            None => false,
        }
    }

    #[test]
    fn the_integration_tests_share_a_folder_on_the_same_hypervisors_as_the_launcher() {
        let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/integration/run.sh")).unwrap();
        for provider in crate::runtime::providers::Provider::ALL.map(crate::runtime::providers::Provider::id) {
            for os in Os::ALL {
                assert_eq!(
                    script_shares(&script, provider, os),
                    shares_folders(true, Target::Vagrant, Some(provider), os),
                    "{provider} on {os:?}: shared_for and shares_folders disagree"
                );
            }
        }
    }

    /// Every computer the launcher ships for, as Rust names its CPU: Apple Silicon and Intel Macs,
    /// Windows on x64 and Arm, Linux on x64 and Arm.
    const ARM: Arch = Arch::Aarch64;
    const X86: Arch = Arch::X86_64;

    #[test]
    fn on_an_arm_computer_the_box_of_a_lab_here_runs_natively_on_every_local_form() {
        for target in [Target::Docker, Target::Vagrant] {
            assert_eq!(box_cpu(ARM, true, target), Cpu::Arm, "{target:?}");
        }
    }

    #[test]
    fn on_an_x86_computer_the_box_takes_the_labs_cpu() {
        for target in [Target::Docker, Target::Vagrant] {
            assert_eq!(box_cpu(X86, true, target), Cpu::Lab, "{target:?}");
        }
    }

    #[test]
    fn on_a_server_or_in_a_cloud_account_the_box_takes_the_labs_cpu_whatever_this_computer_is() {
        for arch in [ARM, X86] {
            for target in [Target::Vagrant, Target::Proxmox, Target::CloudDocker, Target::CloudVm] {
                assert_eq!(box_cpu(arch, false, target), Cpu::Lab, "{arch:?} {target:?}");
            }
        }
    }
}
