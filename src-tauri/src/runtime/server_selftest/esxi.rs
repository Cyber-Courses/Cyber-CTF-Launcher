//! The ESXi self-test: a real throwaway lab on the host, through the exact path a lab takes.
//!
//! First this computer's side: labs on ESXi run from here, through Vagrant's `vmware_esxi`
//! provider (the `vagrant-vmware-esxi` plugin) and VMware's OVF Tool, which uploads each VM to
//! the host. Both are checked up front, as the machine report finds them, and the host's SSH port
//! reached. Then the test lab ([`lab_run::LAB`]) is generated for the `vagrant` target and run by
//! the Isothrum runner with the host's settings as params and its password as the run's
//! connection, as a launch runs one ([`esxi_run`]): the box downloaded and uploaded to the
//! datastore, the VM powered on, its address on the host's network read through VMware Tools, the
//! VM reached over SSH (its provisioning) and a command run inside, then everything destroyed. The
//! teardown always runs, even after a failure, and says what is left when it fails; a test cut
//! short keeps its folder, and the next one removes its VM first. The run is named
//! `cyberctf-server-selftest-<host id>` ([`lab_run::run_id`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::lab_run::{self, Course, SSH, Step, TestRun, run_id};
use super::wait_tcp;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::docker::Check;
use crate::runtime::isothrum::LabRun;
use crate::runtime::lab::esxi::{DATASTORE, HOSTNAME, HOSTPORT, VIRTUAL_NETWORK};
use crate::runtime::launch::{Inputs, esxi_run};
use crate::runtime::providers::{self, Provider};
use crate::runtime::{Runtime, lab, server};

/// The steps of its own the run reports, in order (the frontend shows the same plan).
const IMAGE: Step = ("image", "Upload the test VM to the ESXi datastore");
const VM: Step = ("vm", "Start the test VM");
const ADDRESS: Step = ("address", "The test VM gets an address on the host's network");

/// The steps the test reports, in order (the frontend lists the same: [`super::PLANS`]).
#[cfg(test)]
pub(super) const PLAN: [&str; 8] = [super::CONNECT, lab_run::PREPARE.0, IMAGE.0, VM.0, ADDRESS.0, SSH.0, lab_run::COMMAND.0, lab_run::CLEANUP.0];

/// The test VM's name on the host (Isoloom's `<lab>-<machine>`).
const VM_NAME: &str = "cyberctf-server-selftest-probe";

/// The port group a VM's first adapter goes on when the host's profile names none (the
/// Vagrantfile's default).
const DEFAULT_NETWORK: &str = "VM Network";

/// What this computer has for a lab on ESXi, as the machine report finds it.
pub(super) struct Tools {
    /// The Vagrant plugin `vagrant-vmware-esxi` ready, or why it isn't.
    pub(super) plugin: std::result::Result<(), String>,
    /// VMware's OVF Tool's version, or why it can't be used.
    pub(super) ovftool: std::result::Result<String, String>,
}

/// [`Tools`] on this computer: the ESXi provider's status as the machine report finds it, and
/// `ovftool` (the machine layer's check, handed in).
async fn tools_here(ovftool: impl Future<Output = std::result::Result<String, String>>) -> Tools {
    // A tool installed since the app started is found without a restart.
    crate::platform::env_path::refresh();
    let (status, ovftool) = tokio::join!(providers::status(Provider::VmwareEsxi), ovftool);
    let plugin = match status {
        Some(s) if s.available => Ok(()),
        Some(s) => Err(s.reason.map_or_else(|| "Vagrant plugin `vagrant-vmware-esxi` isn't ready".into(), |r| r.message())),
        None => Err("Vagrant's ESXi provider isn't offered on this computer".into()),
    };
    Tools { plugin, ovftool }
}

/// The ESXi side of the test's first step, after the host's secret: this computer's tools found
/// as the machine report finds them ([`tools_here`], the OVF Tool's check `ovftool`), then the
/// host reached over SSH. Its detail.
pub(super) async fn reach(conn: &server::Connection, ovftool: impl Future<Output = std::result::Result<String, String>>) -> Result<String> {
    reach_with(conn, tools_here(ovftool).await).await
}

/// [`reach`] with `tools`: refused before anything reaches the host when one is missing.
async fn reach_with(conn: &server::Connection, tools: Tools) -> Result<String> {
    let missing: Vec<String> = [tools.plugin.as_ref().err(), tools.ovftool.as_ref().err()].into_iter().flatten().cloned().collect();
    if !missing.is_empty() {
        return Err(Error::Invalid(format!(
            "Labs on ESXi run from this computer through Vagrant's ESXi plugin and VMware's OVF Tool, and this one isn't ready: {}. Set them up in the ESXi setup's tools step, then run the test again.",
            missing.join("; ")
        )));
    }
    let Some(host) = setting(conn, HOSTNAME) else {
        return Err(Error::Invalid("the host's address isn't set".into()));
    };
    let port = setting(conn, HOSTPORT).and_then(|p| p.parse().ok()).unwrap_or(22);
    wait_tcp(&host, port, Duration::from_secs(30)).await?;
    let ovftool = tools.ovftool.unwrap_or_default();
    Ok(format!("{host}:{port} open; vagrant-vmware-esxi and {ovftool} ready"))
}

/// The host's setting `name`, as its labs get it (`lab::esxi_env`'s names).
fn setting(conn: &server::Connection, name: &str) -> Option<String> {
    conn.env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).filter(|v| !v.trim().is_empty())
}

/// The test lab's run on the host, as a launch runs a lab there.
pub(super) struct OnEsxi(LabRun);

impl TestRun for OnEsxi {
    async fn up(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()> {
        self.0.up(Inputs::default(), Some(Provider::VmwareEsxi.id()), &mut |l| log(l)).await
    }
    async fn check(&self) -> Result<Check> {
        self.0.check().await
    }
    async fn down(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()> {
        self.0.down(&mut |l| log(l)).await
    }
}

/// The test lab's run on host `id` (connection `conn`), from its folder `dir`: also how a lab a
/// previous test kept is removed ([`lab_run::clear_kept`]).
pub(super) fn open(dirs: &AppDirs, id: &str, conn: &server::Connection, dir: &Path) -> Result<OnEsxi> {
    Ok(OnEsxi(esxi_run(dirs, dir, &run_id(id), Runtime::Vm, conn)?))
}

/// The test lab written into `work`, generated for Vagrant as a launch on ESXi does, and its run
/// opened, with what the preparation step says.
pub(super) fn prepare(dirs: &AppDirs, id: &str, conn: &server::Connection, work: &Path) -> Result<(OnEsxi, Option<String>)> {
    let run = open(dirs, id, conn, &lab_run::write_lab(work, lab::vagrant_target(Runtime::Vm))?)?;
    Ok((run, Some(format!("VM {VM_NAME}, on port group {}", network_of(conn)))))
}

/// How the test lab comes up on host `conn`.
pub(super) fn course(conn: &server::Connection) -> impl Course + use<> {
    Esxi::new(conn, vagrant_boxes())
}

/// The port group the test VM's first adapter goes on: the first the profile lists.
fn network_of(conn: &server::Connection) -> String {
    setting(conn, VIRTUAL_NETWORK)
        .and_then(|n| n.split(',').next().map(|n| n.trim().to_owned()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DEFAULT_NETWORK.into())
}

/// Vagrant's box store on this computer (`$VAGRANT_HOME/boxes`, else `~/.vagrant.d/boxes`).
fn vagrant_boxes() -> Option<PathBuf> {
    let home = std::env::var_os("VAGRANT_HOME").map(PathBuf::from).or_else(|| crate::platform::home_dir().map(|h| h.join(".vagrant.d")))?;
    Some(home.join("boxes"))
}

/// The box the test VM is built from, as Vagrant's lines name it.
#[derive(Default)]
struct Boxed {
    name: Option<String>,
    version: Option<String>,
    arch: Option<String>,
}

/// How the test lab comes up on ESXi, from Vagrant's and its ESXi plugin's lines: the box added
/// (named with its architecture), ovftool's upload ended by the VM's registration (`VMID`), the
/// VM powered on, then running with an address its VMware Tools report, then `up` succeeding once
/// its provisioning ran over SSH.
struct Esxi {
    /// The port group the VM's first adapter goes on.
    network: String,
    /// The datastore the VM is uploaded to, when the profile names one.
    datastore: Option<String>,
    /// Vagrant's box store, where a box already downloaded is found with its architecture.
    boxes: Option<PathBuf>,
    used: Boxed,
    /// The size of the box's VMware build in the box store, once the box is named and stored:
    /// about what the OVF Tool uploads.
    size: Option<u64>,
    /// The OVF Tool's last error line, which the plugin's own error leaves out.
    ovftool_said: Option<String>,
}

impl Esxi {
    fn new(conn: &server::Connection, boxes: Option<PathBuf>) -> Self {
        Esxi { network: network_of(conn), datastore: setting(conn, DATASTORE), boxes, used: Boxed::default(), size: None, ovftool_said: None }
    }

    /// Notes the box `line` names: Vagrant adding it (`Adding box 'NAME' (vVERSION) for provider:
    /// PROVIDER (ARCH)`), or the plugin's summary (`--- Box : NAME`, `--- Box Ver : VERSION`).
    fn note_box(&mut self, line: &str) {
        if let Some(rest) = line.split_once("Adding box '").map(|(_, r)| r) {
            let Some((name, rest)) = rest.split_once('\'') else { return };
            self.used.name = Some(name.to_owned());
            self.used.version = rest.split_once("(v").and_then(|(_, v)| v.split_once(')')).map(|(v, _)| v.to_owned());
            self.used.arch =
                rest.split_once("for provider:").and_then(|(_, p)| p.rsplit_once('(')).and_then(|(_, a)| a.split_once(')')).map(|(a, _)| a.trim().to_owned());
        } else if let Some((key, value)) = summary(line) {
            match key {
                "Box" => self.used.name = Some(value.to_owned()),
                "Box Ver" => self.used.version = Some(value.to_owned()),
                _ => {}
            }
        }
    }

    /// Notes the size of the box `line` names, once it is in the box store (the plugin names it
    /// after Vagrant downloaded it, before the OVF Tool uploads it).
    fn note_size(&mut self, line: &str) {
        self.note_box(line);
        if self.size.is_none()
            && let (Some(boxes), Boxed { name: Some(name), version: Some(version), arch }) = (self.boxes.as_deref(), &self.used)
        {
            self.size = stored_size(boxes, name, version, arch.as_deref());
        }
    }

    /// The box the VM was built from, with its architecture, and where it went.
    fn box_said(&self) -> Option<String> {
        let Boxed { name: Some(name), version, arch } = &self.used else { return None };
        let arch = arch.clone().or_else(|| stored_arch(self.boxes.as_deref()?, name, version.as_deref()?)).unwrap_or_else(|| "architecture unknown".into());
        let version = version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
        let to = self.datastore.as_deref().map(|d| format!(", uploaded to datastore {d}")).unwrap_or_default();
        Some(format!("Box {name}{version} ({arch}){to}"))
    }
}

/// The ESXi plugin's summary line `--- KEY : VALUE`, as its key and value.
fn summary(line: &str) -> Option<(&str, &str)> {
    let (_, said) = line.split_once("--- ")?;
    let (key, value) = said.split_once(':')?;
    Some((key.trim(), value.trim()))
}

/// The architectures box `name` at `version` is stored for in Vagrant's box store `boxes`, for a
/// VMware provider: the one Vagrant picked when the box was already downloaded.
fn stored_arch(boxes: &Path, name: &str, version: &str) -> Option<String> {
    let arches = vmware_builds(boxes, name, version);
    (!arches.is_empty()).then(|| arches.into_keys().collect::<Vec<_>>().join(" or "))
}

/// The size of box `name` at `version`'s VMware build for `arch` in Vagrant's box store `boxes`
/// (the only one stored, when `arch` isn't known); none when it isn't stored.
fn stored_size(boxes: &Path, name: &str, version: &str, arch: Option<&str>) -> Option<u64> {
    let builds = vmware_builds(boxes, name, version);
    let build = match arch {
        Some(arch) => builds.get(arch)?,
        None if builds.len() == 1 => builds.values().next()?,
        None => return None,
    };
    // A box's build is one folder of files (its disks, its `.vmx`).
    let size: u64 = std::fs::read_dir(build).ok()?.flatten().filter_map(|f| f.metadata().ok()).filter(std::fs::Metadata::is_file).map(|m| m.len()).sum();
    (size > 0).then_some(size)
}

/// Box `name` at `version`'s VMware builds in Vagrant's box store `boxes`: each architecture's
/// folder, by architecture.
fn vmware_builds(boxes: &Path, name: &str, version: &str) -> std::collections::BTreeMap<String, PathBuf> {
    let dir = boxes.join(name.replace('/', "-VAGRANTSLASH-")).join(version);
    let vmware = |arch: &Path| {
        std::fs::read_dir(arch)
            .into_iter()
            .flatten()
            .flatten()
            .map(|p| p.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("vmware")))
    };
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|a| Some((a.file_name().to_string_lossy().into_owned(), vmware(&a.path())?))).collect()
}

impl Course for Esxi {
    const UP: &'static [Step] = &[IMAGE, VM, ADDRESS, SSH];

    fn ends(&mut self, line: &str) -> Option<usize> {
        self.note_size(line);
        if line.trim_start().starts_with("Error:") {
            self.ovftool_said = Some(line.trim().to_owned());
        }
        if summary(line).is_some_and(|(key, _)| key == "VMID") {
            Some(0)
        } else if line.contains("VM has been Powered On") {
            Some(1)
        } else if line.contains("state is now \"running\"") {
            Some(2)
        } else {
            None
        }
    }

    fn size(&self, at: usize) -> Option<u64> {
        (at == 0).then_some(self.size).flatten()
    }

    fn detail(&self, at: usize) -> Option<String> {
        match at {
            0 => self.box_said(),
            2 => Some(format!("On port group {}", self.network)),
            _ => None,
        }
    }

    fn explain(&self, raw: &str) -> Option<(usize, String)> {
        let said = self.ovftool_said.as_deref().unwrap_or_default();
        if said.contains("Duplicate name") || raw.contains("Duplicate name") {
            return Some((
                0,
                format!(
                    "A VM named {VM_NAME} is already on the host, left by an earlier test: delete it in the ESXi host client (Virtual Machines), then run the test again."
                ),
            ));
        }
        if raw.contains("OVF Tool error") || raw.contains("ovftool not found") {
            let why = if said.is_empty() { raw.trim() } else { said };
            let to = self.datastore.as_deref().map(|d| format!(" to datastore {d}")).unwrap_or_default();
            return Some((
                0,
                format!(
                    "The OVF Tool couldn't upload the test VM{to}: it needs the host's HTTPS (port 443) from this computer, and room on the datastore. Details: {why}"
                ),
            ));
        }
        if raw.contains("box") && (raw.contains("could not be found") || raw.contains("doesn't support the provider") || raw.contains("architecture")) {
            return Some((0, format!("Vagrant couldn't get the test VM's box for VMware (it needs internet). Details: {raw}")));
        }
        if raw.contains("Unable to power on VM") {
            return Some((
                1,
                format!("The ESXi host couldn't power on the test VM (not enough free memory on the host, or a box built for another CPU). Details: {raw}"),
            ));
        }
        if raw.contains("timeout waiting for \"running\"") {
            return Some((
                2,
                format!(
                    "The test VM got no address on port group {} within 4 minutes: the port group needs a DHCP server, and the VM's VMware Tools must report its address. Details: {raw}",
                    self.network
                ),
            ));
        }
        if ["Timed out while waiting for the machine to boot", "SSH authentication failed", "The SSH connection was unexpectedly closed"]
            .iter()
            .any(|s| raw.contains(s))
        {
            return Some((
                3,
                format!(
                    "The test VM didn't answer over SSH at its address on port group {}: this computer must reach that network. Details: {raw}",
                    self.network
                ),
            ));
        }
        None
    }

    fn left(&self, e: &Error) -> String {
        format!(
            "Couldn't remove everything the test created on the ESXi host ({e}). Its VM, {VM_NAME}, may still be there: the next test removes it first, or delete it in the ESXi host client."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::lab_run::scripted::*;
    use super::*;

    /// A host's connection with its settings `env` (`lab::esxi_env`'s names).
    fn conn(env: &[(&str, &str)]) -> server::Connection {
        server::Connection {
            id: "esxi1".into(),
            provider: Provider::VmwareEsxi,
            name: "ESXi".into(),
            env: env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            tf_vars: Vec::new(),
            tf_env: Vec::new(),
            region: None,
            auto_stop_hours: None,
        }
    }

    /// The course on a host with datastore `ssd1` and port group `Lab Net`, no box store.
    fn esxi() -> Esxi {
        Esxi::new(&conn(&[(DATASTORE, "ssd1"), (VIRTUAL_NETWORK, "Lab Net, Other")]), None)
    }

    /// Vagrant's and the plugin's lines of a run that went all the way, the box downloaded.
    const ALL_THE_WAY: [&str; 13] = [
        "Building the environment's VMs (Windows machines take a while)…",
        "Bringing machine 'probe' up with 'vmware_esxi' provider...",
        "==> probe: Adding box 'bento/debian-12' (v202510.26.0) for provider: vmware_desktop (amd64)",
        "    probe: Downloading: https://vagrantcloud.com/bento/boxes/debian-12/versions/202510.26.0/providers/vmware_desktop/amd64/vagrant.box",
        "==> probe: --- Box             : bento/debian-12",
        "==> probe: --- Box Ver         : 202510.26.0",
        "Opening VMX source: /tmp/probe.vmx",
        "Transfer Completed",
        "==> probe: --- VMID            : 12",
        "==> probe: --- VM has been Powered On...",
        "==> probe: --- Waiting for state \"running\"",
        "==> probe: --- Success, state is now \"running\"",
        "==> probe: Running provisioner: project...",
    ];

    #[tokio::test]
    async fn a_run_that_works_reports_every_step_of_the_plan_and_tears_down() {
        let (r, seen) = reporter();
        let run = Scripted::new(&ALL_THE_WAY, None);
        let ran = lab_run::test(&r, &run, esxi()).await;
        assert!(ran.result.is_ok() && ran.cleaned);
        assert_eq!(ends(&seen), ["image:ok", "vm:ok", "address:ok", "ssh:ok", "command:ok", "cleanup:ok"]);
        // The steps the plan lists after the connection and the preparation, in its order.
        assert_eq!(ends(&seen), PLAN[2..].iter().map(|s| format!("{s}:ok")).collect::<Vec<_>>());
        assert_eq!(run.calls(), ["up", "check", "down"]);
        // The image step names the box and its architecture (a wrong one would show), and where.
        assert_eq!(detail(&seen, "image", "ok"), "Box bento/debian-12 v202510.26.0 (amd64), uploaded to datastore ssd1");
        assert!(showed(&seen, "image", "Transfer Completed"), "the upload's lines show under its step");
        assert_eq!(detail(&seen, "address", "ok"), "On port group Lab Net");
        assert!(showed(&seen, "ssh", "Running provisioner"), "the provisioning over SSH shows under the SSH step");
        assert_eq!(detail(&seen, "command", "ok"), "Linux x86_64");
    }

    #[tokio::test]
    async fn a_failure_mid_way_fails_its_step_in_plain_words_and_still_cleans_up() {
        let (r, seen) = reporter();
        let raw = "There was an error talking to ESXi.\n  Failed, timeout waiting for \"running\". (exited with code 1)";
        let run = Scripted::new(&ALL_THE_WAY[..10], Some(raw));
        let ran = lab_run::test(&r, &run, esxi()).await;
        let err = ran.result.unwrap_err().to_string();
        assert!(err.starts_with("The test VM got no address on port group Lab Net within 4 minutes"), "{err}");
        assert!(ran.cleaned);
        assert_eq!(ends(&seen), ["image:ok", "vm:ok", "address:fail", "cleanup:ok"]);
        assert_eq!(run.calls(), ["up", "down"], "no command after a failed start, and always the teardown");
    }

    #[tokio::test]
    async fn a_failed_upload_says_what_ovftool_said_and_a_failed_teardown_what_is_left() {
        let (r, seen) = reporter();
        let mut run = Scripted::new(
            &ALL_THE_WAY[..7].iter().copied().chain(["Error: No space left on device"]).collect::<Vec<_>>(),
            Some("There was an OVF Tool error."),
        );
        run.down = Some("vagrant destroy failed");
        let ran = lab_run::test(&r, &run, esxi()).await;
        assert!(!ran.cleaned, "the folder stays so the next test removes the VM first");
        // The run's error is the failed upload; the teardown's step says what is left.
        assert_eq!(ran.result.unwrap_err().to_string(), detail(&seen, "image", "fail"));
        let left = detail(&seen, "cleanup", "fail");
        assert!(left.starts_with("Couldn't remove everything the test created on the ESXi host (vagrant destroy failed)"), "{left}");
        assert!(left.contains(VM_NAME));
        assert_eq!(ends(&seen), ["image:fail", "cleanup:fail"]);
        let why = detail(&seen, "image", "fail");
        assert!(why.contains("to datastore ssd1") && why.ends_with("Details: Error: No space left on device"), "{why}");
    }

    #[test]
    fn the_failures_people_hit_belong_to_their_step() {
        let e = esxi();
        assert_eq!(e.explain("Error: Duplicate name 'cyberctf-server-selftest-probe'").unwrap().0, 0);
        assert!(e.explain("Duplicate name").unwrap().1.contains("delete it in the ESXi host client"));
        assert_eq!(e.explain("The box 'bento/debian-12' could not be found or could not be accessed").unwrap().0, 0);
        assert_eq!(e.explain("There was an error talking to ESXi.\n  Unable to power on VM:\n  …").unwrap().0, 1);
        assert_eq!(e.explain("Timed out while waiting for the machine to boot.").unwrap().0, 3);
        assert!(e.explain("Permission denied (publickey)").is_none());
    }

    #[test]
    fn a_box_already_downloaded_is_named_with_the_architecture_vagrant_stored_it_for() {
        let boxes = std::env::temp_dir().join(format!("cyberctf-esxi-boxes-{}", rand::random::<u32>()));
        let version = boxes.join("bento-VAGRANTSLASH-debian-12").join("202510.26.0");
        std::fs::create_dir_all(version.join("arm64").join("vmware_desktop")).unwrap();
        std::fs::create_dir_all(version.join("amd64").join("virtualbox")).unwrap();
        let mut e = Esxi::new(&conn(&[]), Some(boxes.clone()));
        // No `Adding box` line: the plugin's summary names the box, the store its architecture.
        for line in &ALL_THE_WAY[4..6] {
            assert_eq!(e.ends(line), None);
        }
        assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (arm64)"), "only the VMware build counts");
        std::fs::create_dir_all(version.join("amd64").join("vmware_desktop")).unwrap();
        assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (amd64 or arm64)"));
        std::fs::remove_dir_all(&boxes).unwrap();
        assert_eq!(e.detail(0).as_deref(), Some("Box bento/debian-12 v202510.26.0 (architecture unknown)"));
        assert_eq!(Esxi::new(&conn(&[]), None).detail(0), None, "no box named yet");
        assert_eq!(e.network, DEFAULT_NETWORK);
    }

    #[tokio::test]
    async fn the_upload_carries_the_size_of_the_box_build_it_uploads_once_stored() {
        let boxes = std::env::temp_dir().join(format!("cyberctf-esxi-size-{}", rand::random::<u32>()));
        let version = boxes.join("bento-VAGRANTSLASH-debian-12").join("202510.26.0");
        for (arch, bytes) in [("amd64", 3000), ("arm64", 5)] {
            std::fs::create_dir_all(version.join(arch).join("vmware_desktop")).unwrap();
            std::fs::write(version.join(arch).join("vmware_desktop").join("disk-s001.vmdk"), vec![0u8; bytes]).unwrap();
        }
        std::fs::create_dir_all(version.join("amd64").join("virtualbox")).unwrap();
        std::fs::write(version.join("amd64").join("virtualbox").join("box.vmdk"), vec![0u8; 900]).unwrap();
        let (r, seen) = reporter();
        let run = Scripted::new(&ALL_THE_WAY, None);
        let course = Esxi::new(&conn(&[(DATASTORE, "ssd1")]), Some(boxes.clone()));
        assert!(lab_run::test(&r, &run, course).await.result.is_ok());
        assert_eq!(running_size(&seen, "image"), Some(3000), "the amd64 VMware build Vagrant added, not another");
        assert_eq!(running_size(&seen, "vm"), None);
        // Not stored (yet): no size; with no architecture named, the only VMware build stored.
        assert_eq!(stored_size(&boxes, "bento/debian-12", "202510.1.0", None), None);
        std::fs::remove_dir_all(version.join("arm64")).unwrap();
        assert_eq!(stored_size(&boxes, "bento/debian-12", "202510.26.0", None), Some(3000));
        std::fs::remove_dir_all(&boxes).unwrap();
    }

    fn tools(plugin: std::result::Result<(), &str>, ovftool: std::result::Result<&str, &str>) -> Tools {
        Tools { plugin: plugin.map_err(str::to_owned), ovftool: ovftool.map(str::to_owned).map_err(str::to_owned) }
    }

    #[tokio::test]
    async fn a_missing_plugin_or_ovftool_fails_up_front_before_the_host_is_reached() {
        // No address set: had the check gone on to the host, it would say so instead.
        let host = conn(&[]);
        let err =
            reach_with(&host, tools(Err("Vagrant plugin `vagrant-vmware-esxi` is not installed"), Ok("VMware ovftool 5.1.0"))).await.unwrap_err().to_string();
        assert!(err.starts_with("Labs on ESXi run from this computer") && err.contains("`vagrant-vmware-esxi` is not installed"), "{err}");
        let err = reach_with(&host, tools(Ok(()), Err("VMware's OVF Tool (`ovftool`) wasn't found"))).await.unwrap_err().to_string();
        assert!(err.contains("`ovftool`) wasn't found") && !err.contains("vagrant-vmware-esxi"), "{err}");
        let err = reach_with(&host, tools(Err("no plugin"), Err("no ovftool"))).await.unwrap_err().to_string();
        assert!(err.contains("no plugin; no ovftool"), "{err}");
        // Both there: on to the host.
        assert_eq!(reach_with(&host, tools(Ok(()), Ok("v"))).await.unwrap_err().to_string(), "the host's address isn't set");
    }

    #[tokio::test]
    async fn with_both_tools_the_first_step_reaches_the_hosts_ssh_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let host = conn(&[(HOSTNAME, "127.0.0.1"), (HOSTPORT, &port)]);
        let said = reach_with(&host, tools(Ok(()), Ok("VMware ovftool 5.1.0"))).await.unwrap();
        assert_eq!(said, format!("127.0.0.1:{port} open; vagrant-vmware-esxi and VMware ovftool 5.1.0 ready"));
    }

    /// The test lab generates for Vagrant with the ESXi provider the run drives, the VM named as
    /// the teardown's note says, and its command as the lab's check.
    #[test]
    fn the_test_lab_generates_for_vagrant_on_esxi() {
        let work = std::env::temp_dir().join(format!("cyberctf-esxi-selftest-{}", rand::random::<u32>()));
        let dir = lab_run::write_lab(&work, lab::vagrant_target(Runtime::Vm)).unwrap();
        let generated = lab::vagrant_dir(&dir, Runtime::Vm);
        let vagrantfile = std::fs::read_to_string(generated.join("Vagrantfile")).unwrap();
        assert!(vagrantfile.contains("m.vm.provider \"vmware_esxi\""), "no ESXi provider block");
        assert!(vagrantfile.contains(&format!("v.guest_name = \"{VM_NAME}\"")), "the VM isn't named {VM_NAME}");
        let manifest = std::fs::read_to_string(generated.join("run.json")).unwrap();
        assert!(manifest.contains("\"vmware_esxi\"") && manifest.contains("check-probe"), "{manifest}");
        assert_eq!(lab_run::kept_lab(&work), Some(dir), "a test cut short now is found by the next one");
        assert!(isothrum_runner::RunName::new(&crate::runtime::docker::project(&run_id("0123456789abcdef"))).is_ok());
        std::fs::remove_dir_all(work).unwrap();
    }
}
