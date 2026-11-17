//! Server self-test: prove a host can actually run a lab, not just that it answers.
//!
//! Run by isothrum-machine (`selftest::server`): a real throwaway VM on the player's server, through
//! a runner of its own in the test's folder, brought up, reached over SSH, a command run inside,
//! then always destroyed; a VM an interrupted test left is removed first. What stays here: which
//! server, with its profile and secret (on Proxmox, the node and the jump through it as a launch
//! finds them, and the runner's key the node lets in; on ESXi, this computer's ESXi plugin and OVF
//! Tool checked first), that an AWS account isn't tested, each step in the player's words, and its
//! failures said through the app's error mapping.
//!
//! Progress goes to the UI as one event per step (`running`, then `ok`/`fail`), as the machine
//! self-test's does (`selftest_report`).

use std::future::Future;
use std::path::{Path, PathBuf};

use isothrum_machine::selftest::Progress;
use isothrum_machine::selftest::hypervisor::TestVm;
use isothrum_machine::selftest::server::{self as machine, Esxi, Failure, Found, Key, Proxmox, Server, SignIn, Step, StepProgress};
use isothrum_runner::{ErrorReport, SecretValue};

use super::isothrum::ServerPlace;
use super::lab::esxi::{DATASTORE, HOSTNAME, HOSTPORT, PASSWORD, USERNAME, VIRTUAL_NETWORK};
use super::launch::server_place;
use super::providers::{self, Provider};
use super::selftest_report::{Event, Reporter, StepState};
use super::server;
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The step every host's test starts with: the host and its secret resolved, then reached.
const CONNECT: (&str, &str) = ("connect", "Reach the host");

/// The steps each host's test reports, in order, by provider: what the Server screen lists up
/// front, generated for it by `cargo test` (`SELFTEST_STEPS` in
/// `src/lib/tauri/generated/SelfTestSteps.ts`).
#[cfg(test)]
pub(crate) const PLANS: [(Provider, &[&str]); 2] = [
    (Provider::Proxmox, &["connect", "prepare", "image", "network", "vm", "uplink", "ssh", "command", "cleanup"]),
    (Provider::VmwareEsxi, &["connect", "prepare", "image", "vm", "address", "ssh", "command", "cleanup"]),
];

/// The test VM's host name inside.
const HOSTNAME_INSIDE: &str = "cyberctf-selftest";
/// The port group a VM's card goes on when the host's profile names none (the plugin's default).
const DEFAULT_NETWORK: &str = "VM Network";

/// One self-test per host at a time in this process: two runs would share the test's run on the
/// server and its folder, each tearing the other's down. A second one waits for the first.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The test's own folder for host `id`: the runner's home that records its VM, and its project.
fn work_dir(dirs: &AppDirs, id: &str) -> Result<PathBuf> {
    Ok(dirs.data()?.join("selftest").join("server").join(id))
}

/// The test VM's name on host `id`: the same at every test of the host, so a VM an earlier test
/// left is found under it, and another host's (or another computer's) is another.
fn vm_name(id: &str) -> String {
    format!("cyberctf-server-selftest-{id}")
}

/// Runs a real-VM self-test against host `id`, streaming each step to `sink` (the
/// `server_selftest` command wraps the webview's channel in one). `ovftool` checks this computer's
/// OVF Tool (its version, or why it can't be used), called only for an ESXi host: the machine
/// layer's check, which the runtime doesn't name.
pub(crate) async fn run<F>(dirs: &AppDirs, id: &str, ovftool: impl FnOnce() -> F, sink: impl Fn(Event) + Send + Sync + 'static) -> Result<()>
where
    F: Future<Output = std::result::Result<String, String>>,
{
    let _alone = ONE_AT_A_TIME.lock().await;
    let r = Reporter::new(sink);
    r.send(CONNECT.0, CONNECT.1, StepState::Running, None);
    let target = match target(dirs, id, ovftool).await {
        Ok(target) => target,
        Err(e) => {
            r.send(CONNECT.0, CONNECT.1, StepState::Fail, Some(e.to_string()));
            return Err(e);
        }
    };
    let target = match target {
        Ok(target) => target,
        // Resolved, but not a server: nothing is created to test it.
        Err(c) => {
            r.send(CONNECT.0, CONNECT.1, StepState::Ok, Some(c.name));
            return Err(Error::Invalid(if c.provider == Provider::Aws {
                "the VM test is for server hosts; AWS labs are billed, so it isn't run as a test".into()
            } else {
                "the VM test runs only on a Proxmox or ESXi host".into()
            }));
        }
    };
    test(&work_dir(dirs, id)?, &target, &r).await
}

/// What the test runs on, and how its steps are said.
pub(crate) struct Target {
    server: Server,
    words: Words,
}

/// How the test's steps are said on one server.
struct Words {
    kind: Kind,
    /// The test VM's name on the server.
    vm: String,
    /// What the first step says once the host answers.
    reached: String,
    /// What the preparation says once done.
    prepared: Option<String>,
}

/// The kind of server, with what its steps name.
enum Kind {
    /// Proxmox VE, the test VM on `bridge`.
    Proxmox { bridge: String },
    /// ESXi, the test VM on port group `network`, uploaded to `datastore` when the profile names one.
    Esxi { network: String, datastore: Option<String> },
}

/// Host `id` as the test runs on it, its secret resolved: on Proxmox, with the node and the jump
/// a launch would use; on ESXi, once this computer's plugin and OVF Tool (`ovftool`) are found.
/// The host's connection, for a host the test doesn't run on.
async fn target<F>(dirs: &AppDirs, id: &str, ovftool: impl FnOnce() -> F) -> Result<std::result::Result<Target, server::Connection>>
where
    F: Future<Output = std::result::Result<String, String>>,
{
    let c = server::connection(dirs, id)?;
    match c.provider {
        Provider::Proxmox => {
            let place = Box::pin(server_place(dirs, id, &c)).await?;
            proxmox_target(&c.name, &place, &vm_name(id)).map(Ok)
        }
        Provider::VmwareEsxi => {
            let ovftool = tools_ready(tools_here(ovftool()).await)?;
            esxi_target(&c.env, &ovftool, &vm_name(id)).map(Ok)
        }
        _ => Ok(Err(c)),
    }
}

/// The test on Proxmox server `name` where `place` puts a launch's lab, its VM named `vm`.
pub(crate) fn proxmox_target(name: &str, place: &ServerPlace, vm: &str) -> Result<Target> {
    let var = |k: &str| place.vars.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let endpoint = var("proxmox_endpoint").ok_or_else(|| Error::Invalid("the server's address isn't set".into()))?;
    let sign_in = match var("proxmox_api_token") {
        Some(token) => SignIn::Token(SecretValue::new(token)),
        None => SignIn::Password {
            user: var("proxmox_username").unwrap_or_else(|| "root@pam".into()),
            password: SecretValue::new(var("proxmox_password").unwrap_or_default()),
        },
    };
    // The profile's own node wins over the one discovered.
    let node = var("node").or_else(|| place.node.clone()).ok_or_else(|| Error::Invalid("the server's node isn't known".into()))?;
    let mut p = Proxmox::new(endpoint, sign_in, node);
    p.insecure = var("proxmox_insecure").as_deref() == Some("true");
    p.bridge = var("uplink_bridge").unwrap_or_else(|| p.bridge.clone());
    p.datastore = var("datastore");
    p.image_datastore = var("image_datastore");
    p.snippets_datastore = var("snippets_datastore");
    p.node_ssh_user = var("proxmox_ssh_username");
    p.node_ssh_key_file = var(super::lab::NODE_KEY).map(PathBuf::from);
    p.node_address = var("proxmox_ssh_address");
    // The runner's key, which the lab's VMs and the node (for the jump) let in.
    p.key = var("ssh_private_key_file").zip(var("ssh_public_key")).map(|(private, public)| Key::new(private, public));
    p.jump.clone_from(&place.jump);
    let words = Words {
        kind: Kind::Proxmox { bridge: p.bridge.clone() },
        vm: vm.to_owned(),
        reached: name.to_owned(),
        prepared: place.node.as_ref().map(|n| format!("on node {n}")),
    };
    Ok(Target { server: Server::Proxmox(p), words })
}

/// The test on the ESXi host whose settings are `env` (`lab::esxi_env`'s names), this computer's
/// OVF Tool at version `ovftool`, its VM named `vm`.
fn esxi_target(env: &[(String, String)], ovftool: &str, vm: &str) -> Result<Target> {
    let setting = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_owned()).filter(|v| !v.is_empty());
    let host = setting(HOSTNAME).ok_or_else(|| Error::Invalid("the host's address isn't set".into()))?;
    let mut e = Esxi::new(host, setting(USERNAME).unwrap_or_else(|| "root".into()), SecretValue::new(setting(PASSWORD).unwrap_or_default()));
    e.port = setting(HOSTPORT).and_then(|p| p.parse().ok()).unwrap_or(22);
    e.datastore = setting(DATASTORE);
    e.network = setting(VIRTUAL_NETWORK).and_then(|n| n.split(',').next().map(|n| n.trim().to_owned())).filter(|n| !n.is_empty());
    let network = e.network.clone().unwrap_or_else(|| DEFAULT_NETWORK.into());
    let kind = Kind::Esxi { network: network.clone(), datastore: e.datastore.clone() };
    let server = Server::Esxi(e);
    let words = Words {
        reached: format!("{} open; vagrant-vmware-esxi and {ovftool} ready", server.address()),
        prepared: Some(format!("VM {vm}, on port group {network}")),
        kind,
        vm: vm.to_owned(),
    };
    Ok(Target { server, words })
}

/// What this computer has for a lab on ESXi, as the machine report finds it.
struct Tools {
    /// The Vagrant plugin `vagrant-vmware-esxi` ready, or why it isn't.
    plugin: std::result::Result<(), String>,
    /// VMware's OVF Tool's version, or why it can't be used.
    ovftool: std::result::Result<String, String>,
}

/// [`Tools`] on this computer: the ESXi provider's status as the machine report finds it, and
/// `ovftool` (the machine layer's check, handed in).
async fn tools_here(ovftool: impl Future<Output = std::result::Result<String, String>>) -> Tools {
    // A tool installed since the app started is found without a restart.
    crate::platform::env_path::refresh().await;
    let (status, ovftool) = tokio::join!(providers::status(Provider::VmwareEsxi), ovftool);
    let plugin = match status {
        Some(s) if s.available => Ok(()),
        Some(s) => Err(s.reason.map_or_else(|| "Vagrant plugin `vagrant-vmware-esxi` isn't ready".into(), |r| providers::said(&r))),
        None => Err("Vagrant's ESXi provider isn't offered on this computer".into()),
    };
    Tools { plugin, ovftool }
}

/// The OVF Tool's version when both tools are ready; refused before anything reaches the host
/// when one is missing.
fn tools_ready(tools: Tools) -> Result<String> {
    let missing: Vec<String> = [tools.plugin.as_ref().err(), tools.ovftool.as_ref().err()].into_iter().flatten().cloned().collect();
    if !missing.is_empty() {
        return Err(Error::Invalid(format!(
            "Labs on ESXi run from this computer through Vagrant's ESXi plugin and VMware's OVF Tool, and this one isn't ready: {}. Set them up in the ESXi setup's tools step, then run the test again.",
            missing.join("; ")
        )));
    }
    Ok(tools.ovftool.unwrap_or_default())
}

/// The test on `target`, its folder `work`, each step said through `r`.
pub(crate) async fn test(work: &Path, target: &Target, r: &Reporter) -> Result<()> {
    // A tool installed since the app started (Terraform, Vagrant) is found without a restart.
    crate::platform::env_path::refresh().await;
    let cx = providers::machine_selftest();
    let vm = TestVm::new(&target.words.vm, HOSTNAME_INSIDE);
    let ran = Box::pin(machine::run(&cx, work, &target.server, &vm, &|p| say(r, &target.words, work, p))).await;
    let passed = ran.passed.map_err(|why| error(&target.words, work, why));
    match ran.cleaned_up {
        Some(Err(why)) => kept(work, passed.and(Err(Error::Invalid(left(&target.words, &why))))),
        _ => passed,
    }
}

/// Says one step's `progress` through `r`, in the player's words (`work`: the test's folder).
fn say(r: &Reporter, words: &Words, work: &Path, progress: StepProgress) {
    match progress {
        Progress::Running(step) => {
            let (id, label) = named(&words.kind, step);
            r.send(id, label, StepState::Running, None);
        }
        Progress::Passed(Step::Prepare, Found::Removing) => {
            r.progress("prepare", "Prepare a test VM definition", "Removing the VM a previous test left on the host…");
        }
        Progress::Passed(step, Found::Line { text, bytes }) => {
            let (id, label) = named(&words.kind, step);
            r.progress_sized(id, label, &text, bytes);
        }
        Progress::Passed(step, found) => {
            let (id, label) = named(&words.kind, step);
            r.send(id, label, StepState::Ok, detail(words, step, found));
        }
        Progress::Skipped(step, _) => {
            let (id, label) = named(&words.kind, step);
            r.send(id, label, StepState::Skip, None);
        }
        Progress::Failed(step, why) => {
            let (id, label) = named(&words.kind, step);
            let said = if step == Step::Down { left(words, &why) } else { error(words, work, why).to_string() };
            r.send(id, label, StepState::Fail, Some(said));
        }
        _ => {}
    }
}

/// A step's id and label on a server of `kind`, as the Server screen shows it.
fn named(kind: &Kind, step: Step) -> (&'static str, &'static str) {
    let esxi = matches!(kind, Kind::Esxi { .. });
    match step {
        Step::Host => CONNECT,
        Step::Prepare => ("prepare", "Prepare the test lab"),
        Step::Image if esxi => ("image", "Upload the test VM to the ESXi datastore"),
        Step::Image => ("image", "Download the Debian cloud image"),
        Step::Network => ("network", "Create the test's lab network"),
        Step::Vm if esxi => ("vm", "Start the test VM"),
        Step::Vm => ("vm", "Start the test VMs"),
        Step::Address if esxi => ("address", "The test VM gets an address on the host's network"),
        Step::Address => ("uplink", "The lab's router gets an address on the uplink"),
        Step::Ssh => ("ssh", "Reach the test VM over SSH"),
        Step::Exec => ("command", "Run a command inside the test VM"),
        _ => ("cleanup", "Remove everything the test created"),
    }
}

/// What a step that passed says.
fn detail(words: &Words, step: Step, found: Found) -> Option<String> {
    match (step, found, &words.kind) {
        (Step::Host, _, _) => Some(words.reached.clone()),
        (Step::Prepare, _, _) => words.prepared.clone(),
        (Step::Image, Found::Box { name, version, arch }, Kind::Esxi { datastore, .. }) => {
            let version = version.map(|v| format!(" v{v}")).unwrap_or_default();
            let arch = arch.unwrap_or_else(|| "architecture unknown".into());
            let to = datastore.as_deref().map(|d| format!(", uploaded to datastore {d}")).unwrap_or_default();
            Some(format!("Box {name}{version} ({arch}){to}"))
        }
        (Step::Address, _, Kind::Esxi { network, .. }) => Some(format!("On port group {network}")),
        (Step::Exec, Found::Uname(said), _) => Some(said),
        (Step::Down, _, _) => Some("Nothing of the test is left on the server.".into()),
        _ => None,
    }
}

/// Why a step failed, in the player's terms: the failures people hit with what to do, the
/// runner's other failures through the app's error mapping; what an earlier test left that
/// couldn't be removed, with where its state is kept (`work`).
fn error(words: &Words, work: &Path, why: Failure) -> Error {
    let details = |r: &ErrorReport| r.message.trim().to_owned();
    let invalid = Error::Invalid;
    match (why, &words.kind) {
        (Failure::Unreachable(address), _) => invalid(format!("no answer on {address}")),
        (Failure::Leftover(report), _) => kept_error(work, Err(crate::runtime::isothrum::errors::coded(&report))),
        (Failure::NoAddress(_), Kind::Proxmox { bridge }) => invalid(format!(
            "The test VM got no address on bridge {bridge}: the bridge has no DHCP server reachable from the VMs (or no internet). Use an uplink bridge with DHCP and internet."
        )),
        (Failure::NoAddress(r), Kind::Esxi { network, .. }) => invalid(format!(
            "The test VM got no address on port group {network} within 4 minutes: the port group needs a DHCP server, and the VM's VMware Tools must report its address. Details: {}",
            details(&r)
        )),
        (Failure::NoBridge(r), _) => invalid(format!(
            "A test VM couldn't start because its network bridge doesn't exist on the node. If it is the lab network's, the node doesn't load its SDN configuration: add `source /etc/network/interfaces.d/*` at the end of /etc/network/interfaces, then run `ifreload -a`. Details: {}",
            details(&r)
        )),
        (Failure::ImageDownload(r), _) => invalid(format!(
            "The server couldn't download the Debian cloud image (it needs internet, and a storage with 'iso' content). Details: {}",
            details(&r)
        )),
        (Failure::NameTaken, _) => invalid(format!(
            "A VM named {} is already on the host, left by an earlier test: delete it in the ESXi host client (Virtual Machines), then run the test again.",
            words.vm
        )),
        (Failure::Upload(said), Kind::Esxi { datastore, .. }) => {
            let to = datastore.as_deref().map(|d| format!(" to datastore {d}")).unwrap_or_default();
            invalid(format!(
                "The OVF Tool couldn't upload the test VM{to}: it needs the host's HTTPS (port 443) from this computer, and room on the datastore. Details: {said}"
            ))
        }
        (Failure::NoBox(r), _) => invalid(format!("Vagrant couldn't get the test VM's box for VMware (it needs internet). Details: {}", details(&r))),
        (Failure::PowerOn(r), _) => invalid(format!(
            "The ESXi host couldn't power on the test VM (not enough free memory on the host, or a box built for another CPU). Details: {}",
            details(&r)
        )),
        (Failure::NoSsh(r), kind) => {
            let on = match kind {
                Kind::Proxmox { bridge } => format!("bridge {bridge}"),
                Kind::Esxi { network, .. } => format!("port group {network}"),
            };
            invalid(format!("The test VM didn't answer over SSH at its address on {on}: this computer must reach that network. Details: {}", details(&r)))
        }
        (Failure::Command { output, .. }, _) => invalid(format!("the command inside the test VM failed: {}", output.trim())),
        (Failure::Runner(r), _) => crate::runtime::isothrum::errors::coded(&r),
        (other, _) => invalid(other.to_string()),
    }
}

/// What a teardown that failed with `why` leaves on the server, and how to remove it.
fn left(words: &Words, why: &Failure) -> String {
    let e = why.report().map_or_else(|| why.to_string(), |r| crate::runtime::isothrum::errors::coded(r).to_string());
    match &words.kind {
        Kind::Proxmox { .. } => format!(
            "Couldn't remove everything the test created on the server ({e}). Its VM ({}) and its lab network may still be there: the next test removes them first, or delete them in the Proxmox web UI.",
            words.vm
        ),
        Kind::Esxi { .. } => format!(
            "Couldn't remove everything the test created on the ESXi host ({e}). Its VM, {}, may still be there: the next test removes it first, or delete it in the ESXi host client.",
            words.vm
        ),
    }
}

/// `result`, failed with a note that the test VM is still on the host and where its state is kept.
fn kept(work: &Path, result: Result<()>) -> Result<()> {
    Err(kept_error(work, result))
}

/// [`kept`]'s error.
fn kept_error(work: &Path, result: Result<()>) -> Error {
    let note = format!(
        "the test VM couldn't be removed from the host. Its state is kept in {}; the next test removes it first, or delete the VM on the host yourself",
        work.display()
    );
    Error::Invalid(match result {
        Ok(()) => note,
        Err(e) => format!("{e}; {note}"),
    })
}

#[cfg(test)]
mod tests;
