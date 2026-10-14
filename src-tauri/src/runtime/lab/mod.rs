//! A lab's Isoloom side: every lab describes itself with an `isoloom.yml` at its root, and the
//! launcher generates the files of the target it runs on under the lab's output folder
//! (Compose, Vagrant, Terraform) right before running them. Nothing is read from the lab
//! besides the spec and the files it names, so a lab is portable to every target Isoloom
//! supports.
//!
//! Each installed lab is an Isoloom *instance* (a number, kept in `.cyberctf-instance`): its
//! Docker networks move to their own blocks and its names get a suffix, so two labs that
//! declare the same addresses run at once. The generated files then live in `.isoloom-<n>/`.
//!
//! This module is the launcher's one door to Isoloom (`isoloom_core` is used nowhere else): the
//! spec, the generated files, the resolved snapshot, the checks, the host's readiness and the
//! registry go through it, and so do Isoloom's path and naming conventions (`names`). The
//! launcher's own conventions sit on top: its overrides of the generated files (`overrides`,
//! applied after Isoloom's defaults), the variables it gives Isoloom's Terraform modules from its
//! hosts and cloud accounts (`vars`), and whether it records its labs in Isoloom's registry
//! (`registry`, a setting).

use std::collections::HashMap;
use std::path::Path;

use isoloom_core::model::Protocol;

use super::{Interface, Network};
use crate::error::{Error, Result};

mod names;
mod overrides;
pub mod registry;
mod vars;

pub use names::*;
pub use registry::instances_elsewhere as registry_instances;
pub use vars::{esxi_env, host_vars, run_vars};

// What the rest of the launcher uses of Isoloom's types: through here, like its functions.
pub use isoloom_core::checks;
pub use isoloom_core::generate::StartPlan;
pub use isoloom_core::model::{CloudProvider, CloudServices};
pub use isoloom_core::{Arch, Spec, Target};

/// The lab's spec, checked (fields and the files it names), with this machine's Isoloom
/// defaults applied (its image table).
pub fn spec(dir: &Path) -> Result<Spec> {
    let spec = isoloom_core::load(dir).map_err(|e| Error::Invalid(load_error(&e.to_string())))?;
    let problems: Vec<String> = isoloom_core::validate(&spec).into_iter().chain(isoloom_core::validate_files(&spec, dir)).map(|p| p.to_string()).collect();
    if !problems.is_empty() {
        return Err(Error::Invalid(format!("this lab's isoloom.yml has mistakes:\n{}", problems.join("\n"))));
    }
    Ok(defaults(dir)?.images.apply(&spec))
}

/// A spec as written, unchecked (what a launch dialog previews): `isoloom.yml` in `dir`.
pub fn load(dir: &Path) -> Result<Spec> {
    isoloom_core::load(dir).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// A spec from its text, unchecked.
pub fn parse(text: &str) -> Result<Spec> {
    isoloom_core::parse(text).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// A spec this build can't read. Fields or values it doesn't know mean the lab uses a newer
/// format than this launcher (labs adopt spec additions before every player has updated), so
/// say what to do instead of showing only the parser's message.
fn load_error(e: &str) -> String {
    if e.contains("unknown field") || e.contains("unknown variant") {
        format!("This lab needs a newer version of Cyber CTF. Update it (Settings > About), then start the lab again.\n(isoloom.yml: {e})")
    } else {
        format!("this lab's isoloom.yml: {e}")
    }
}

/// This machine's Isoloom defaults for the lab: the user's `~/.isoloom/defaults.yml`, the
/// lab's own `isoloom.defaults.yml`, then `ISOLOOM_*` in the environment (the same hierarchy
/// the `isoloom` CLI reads), so a user's image table or cloud regions hold here too.
fn defaults(dir: &Path) -> Result<isoloom_core::defaults::Defaults> {
    isoloom_core::defaults::load(dir, None, &[]).map(|r| r.defaults).map_err(|e| Error::Invalid(format!("this machine's Isoloom defaults: {e}")))
}

/// The lab's Isoloom instance number (see the module), when it has one.
pub const INSTANCE_MARKER: &str = ".cyberctf-instance";

pub fn instance(dir: &Path) -> Option<u8> {
    std::fs::read_to_string(dir.join(INSTANCE_MARKER)).ok()?.trim().parse().ok().filter(|n| (1..=isoloom_core::instance::MAX).contains(n))
}

/// Gives the lab an instance number if it has none: the lowest one no other installed lab
/// under `labs` holds, nor another environment in Isoloom's registry (one run with the CLI),
/// whose Docker blocks overlap none of `in_use` (the subnets of the Docker networks already
/// on this machine). An instance moves the spec's blocks by its number, so two labs with
/// different numbers can still land on one subnet; Docker would refuse the second. Returns
/// the lab's number.
pub fn ensure_instance(labs: &Path, dir: &Path, in_use: &[String], elsewhere: &[u8]) -> Result<u8> {
    let _picking = lock_instances(labs)?;
    match instance(dir) {
        Some(n) => Ok(n),
        None => pick_instance(labs, dir, in_use, elsewhere),
    }
}

/// [`ensure_instance`] at a start, for a lab that isn't running: its number stays unless its
/// blocks overlap one of `in_use` now (picked while Docker was down and couldn't say what it
/// had, or taken since), in which case it moves to a free one. Docker would otherwise refuse
/// its networks ("Pool overlaps").
pub fn recheck_instance(labs: &Path, dir: &Path, in_use: &[String], elsewhere: &[u8]) -> Result<u8> {
    // A lab without a readable spec can't start: nothing to place, nothing written.
    let spec = spec(dir)?;
    let _picking = lock_instances(labs)?;
    match instance(dir) {
        Some(n) if blocks_clear(Some(&spec), n, &ranges(in_use)) => Ok(n),
        _ => pick_instance(labs, dir, in_use, elsewhere),
    }
}

/// Held (an OS lock on `<labs>/.instances.lock`, across processes) while an instance is picked
/// and saved: two labs installing at once read the same free number otherwise.
fn lock_instances(labs: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(labs)?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(labs.join(".instances.lock"))?;
    file.lock()?;
    Ok(file)
}

/// The lowest instance number free for lab `dir` (see [`ensure_instance`]), saved in its marker.
fn pick_instance(labs: &Path, dir: &Path, in_use: &[String], elsewhere: &[u8]) -> Result<u8> {
    let mut taken: Vec<u8> = std::fs::read_dir(labs)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p != dir).filter_map(|p| instance(&p)).collect())
        .unwrap_or_default();
    taken.extend(elsewhere);
    let spec = spec(dir).ok();
    let used = ranges(in_use);
    let n = (1..=isoloom_core::instance::MAX)
        .find(|n| !taken.contains(n) && blocks_clear(spec.as_ref(), *n, &used))
        .ok_or_else(|| Error::Invalid("no free instance for this lab: 99 labs or networks already take its room on this machine".into()))?;
    crate::atomic_file::write(&dir.join(INSTANCE_MARKER), n.to_string())?;
    Ok(n)
}

/// The IPv4 ranges of `cidrs`.
fn ranges(cidrs: &[String]) -> Vec<(u32, u32)> {
    cidrs.iter().filter_map(|c| ipv4_range(c)).collect()
}

/// Whether the Docker blocks of `spec` as instance `n` overlap none of `used`. A lab whose spec
/// can't be read has no blocks to collide.
fn blocks_clear(spec: Option<&Spec>, n: u8, used: &[(u32, u32)]) -> bool {
    let Some(spec) = spec else { return true };
    let Ok(as_n) = isoloom_core::instance::apply(spec, n) else { return false };
    let snapshot = isoloom_core::resolved::resolve_with(&as_n, Some(n));
    snapshot["networks"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, net)| net["docker_cidr"].as_str().and_then(ipv4_range))
        .all(|(lo, hi)| used.iter().all(|&(a, b)| hi < a || b < lo))
}

/// An IPv4 CIDR as its first and last address.
pub fn ipv4_range(cidr: &str) -> Option<(u32, u32)> {
    let (ip, len) = cidr.split_once('/')?;
    let ip: u32 = ip.parse::<std::net::Ipv4Addr>().ok()?.into();
    let len: u32 = len.parse().ok().filter(|l| *l <= 32)?;
    let size = if len == 0 { u32::MAX } else { (1u32 << (32 - len)) - 1 };
    let lo = ip & !size;
    Some((lo, lo | size))
}

/// The spec as this lab's instance of it (what `prepare` generates from): its own name and
/// Docker blocks when the lab has an instance number.
pub fn instanced(dir: &Path) -> Result<Spec> {
    let spec = spec(dir)?;
    match instance(dir) {
        Some(n) => isoloom_core::instance::apply(&spec, n).map_err(|e| Error::Invalid(format!("this lab as instance {n}: {e}"))),
        None => Ok(spec),
    }
}

/// An observer the lab puts beside itself (`tools:` in its spec): a toolbox or a capture box
/// on every network, outside the lab's contract.
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabTool")]
pub struct Observer {
    pub name: String,
    pub image: Option<String>,
    /// Its address on each lab network, where the lab runs.
    pub addresses: Vec<Interface>,
    /// The loopback port its web UI is published on, when it has one.
    pub publish: Option<u16>,
}

/// The lab's observers at their addresses: the container ones when the lab runs as
/// containers (`docker`), else the ones the spec writes.
pub fn tools(spec: &Spec, docker: bool) -> Vec<Observer> {
    let snapshot = isoloom_core::resolved::resolve(spec);
    let key = if docker { "docker_addresses" } else { "addresses" };
    snapshot["tools"]
        .as_object()
        .map(|tools| {
            tools
                .iter()
                .map(|(name, t)| Observer {
                    name: name.clone(),
                    image: t["image"].as_str().map(str::to_string),
                    addresses: t[key]
                        .as_object()
                        .map(|a| a.iter().filter_map(|(net, ip)| ip.as_str().map(|ip| Interface { network: net.clone(), ip: ip.to_string() })).collect())
                        .unwrap_or_default(),
                    publish: t["publish"].as_u64().and_then(|p| u16::try_from(p).ok()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Generates `target`'s files into the lab (overwriting the previous ones), and returns the
/// spec as this lab's instance of it. Fails with Isoloom's own reason when the lab can't run
/// there (e.g. Windows machines on a target without Windows images). Isoloom's defaults are
/// applied to the files, then the launcher's own overrides (see `overrides`).
pub fn prepare(dir: &Path, target: Target) -> Result<Spec> {
    let spec = instanced(dir)?;
    let n = instance(dir);
    let files = isoloom_core::generate(&spec, target).map_err(|e| Error::Invalid(format!("this lab can't run there: {e}")))?;
    let files = isoloom_core::defaults::apply_to_files(files, &defaults(dir)?);
    let files = overrides::apply(files, &launcher_overrides(dir, target));
    for f in files {
        let (path, contents) = match n {
            Some(n) => isoloom_core::instance::relocate(&f.path, &f.contents, n),
            None => (f.path, f.contents),
        };
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Ok(spec)
}

/// The launcher's overrides of a lab's files for `target`: on local Docker, its Compose project
/// (`cyberctf-<lab id>`, the lab's folder being its id).
fn launcher_overrides(dir: &Path, target: Target) -> overrides::Overrides {
    let id = dir.file_name().and_then(|n| n.to_str()).filter(|_| target == Target::Docker);
    overrides::Overrides { compose_project: id.map(super::docker::project) }
}

/// Each machine's lab interfaces and the lab's networks, from Isoloom's resolved snapshot of
/// the spec: addresses are static (declared in the lab), so the diagram of a VM lab comes from
/// here, without reaching into the guests or reading the generated files.
pub fn topology(spec: &Spec) -> (HashMap<String, Vec<Interface>>, Vec<Network>) {
    let snapshot = isoloom_core::resolved::resolve(spec);
    let networks: Vec<Network> = snapshot["networks"]
        .as_object()
        .map(|nets| {
            nets.iter()
                .map(|(name, n)| Network {
                    name: name.clone(),
                    subnet: n["cidr"].as_str().unwrap_or_default().to_string(),
                    // No internet declared: its machines have no route out.
                    internal: !n["internet"].as_bool().unwrap_or(true),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut ifaces: HashMap<String, Vec<Interface>> = HashMap::new();
    if let Some(machines) = snapshot["machines"].as_object() {
        for (name, m) in machines {
            let list: Vec<Interface> = m["addresses"]
                .as_object()
                .map(|a| a.iter().filter_map(|(net, ip)| ip.as_str().map(|ip| Interface { network: net.clone(), ip: ip.to_string() })).collect())
                .unwrap_or_default();
            ifaces.insert(name.clone(), list);
        }
    }
    (ifaces, networks)
}

/// The lab's `message:` (where to start, the first step) with its placeholders filled from
/// the resolved spec (`spec` as `prepare` returned it); shown once the lab is up. None when
/// the lab has none, or it names a value the spec doesn't have (the lab's own mistake, not
/// worth failing a launch over).
pub fn message(dir: &Path, spec: &Spec, target: Target) -> Option<String> {
    message_at(dir, spec, target, &[])
}

/// `message` with the host ports the lab really got: (machine, container port, protocol, host
/// port), the protocol `tcp` or `udp`. A local container lab publishes on ports picked at its
/// first start, not the `publish:` values its spec declares, so a
/// `{{ machines.web.services.0.publish }}` must say where it answers. Addresses are the ones
/// machines have on `target` (their Docker blocks on the Compose targets).
pub fn message_at(dir: &Path, spec: &Spec, target: Target, published: &[(String, u16, String, u16)]) -> Option<String> {
    let text = spec.message.as_deref()?;
    let snapshot = isoloom_core::resolved::on_target(isoloom_core::resolved::resolve_with(spec, instance(dir)), target);
    let published: Vec<(String, u16, Protocol, u16)> =
        published.iter().map(|(m, port, proto, host)| (m.clone(), *port, if proto == "udp" { Protocol::Udp } else { Protocol::Tcp }, *host)).collect();
    let snapshot = isoloom_core::resolved::with_published(snapshot, &published);
    // A placeholder naming a value this run doesn't have (a port the lab never published on this
    // target, a port not pinned yet) fills as nothing: the player got `http://localhost:/`.
    if placeholders(text).any(|path| isoloom_core::resolved::lookup(&snapshot, path).is_none_or(|v| v.is_null() || v.as_str() == Some(""))) {
        return None;
    }
    isoloom_core::resolved::fill(text, &snapshot).ok().map(|m| m.trim_end().to_string())
}

/// The dotted paths of a message's `{{ ... }}` placeholders.
fn placeholders(text: &str) -> impl Iterator<Item = &str> {
    text.split("{{").skip(1).filter_map(|after| after.split_once("}}").map(|(path, _)| path.trim()))
}

/// The memory the lab's machines ask for, in MB.
pub fn memory_mb(spec: &Spec) -> u32 {
    isoloom_core::totals(spec).memory_mb
}

/// Why the lab's VMs can't run on Vagrant's QEMU provider (its networks), if they can't.
pub fn qemu_refusal(spec: &Spec) -> Option<String> {
    isoloom_core::refusal_on(spec, Target::Vagrant, Some("qemu"))
}

/// How the lab's containers start, from its Compose file's text (Isoloom's start plan).
pub fn start_plan(compose: &str) -> Option<StartPlan> {
    isoloom_core::generate::start_plan(compose).ok()
}

/// The check runner Isoloom writes for `machine`'s `exec` checks (under the Compose file's
/// `checks/`).
pub fn exec_runner(machine: &str) -> String {
    isoloom_core::generate::exec_runner(machine)
}

/// Isoloom's view of this host: each target (each cloud for the cloud ones), ready or not, and
/// why. Runs the tools, so call it off the async runtime.
pub fn host_readiness() -> Vec<isoloom_core::host::Readiness> {
    isoloom_core::host::all()
}

/// What a cloud-services lab's module needs on this machine besides Terraform
/// (`cloud.requires`), checked here.
#[derive(Debug, PartialEq)]
pub enum Requirements {
    /// It names nothing.
    None,
    /// All there.
    Met(Vec<String>),
    /// What is missing, as Isoloom words it.
    Missing(Vec<String>),
}

/// [`Requirements`] of a spec, as `isoloom run` checks them: a command name looked up on the
/// PATH (with `PATHEXT` on Windows), a shell command run (the lab's own code, as its Terraform
/// module is) with `sh -c`, or PowerShell on Windows.
pub async fn cloud_requirements(spec: &Spec) -> Requirements {
    if !spec.cloud.as_ref().is_some_and(|c| !c.requires.is_empty()) {
        return Requirements::None;
    }
    let spec = spec.clone();
    let checked = tokio::task::spawn_blocking(move || isoloom_core::host::requirements(&spec, true)).await.ok().flatten();
    requirements_of(checked)
}

/// [`Requirements`] from Isoloom's readiness (each note an entry, `! ` before a missing one).
fn requirements_of(checked: Option<isoloom_core::host::Readiness>) -> Requirements {
    let Some(r) = checked else { return Requirements::None };
    if r.ready {
        return Requirements::Met(r.notes);
    }
    Requirements::Missing(r.notes.iter().filter_map(|n| n.strip_prefix("! ")).map(str::to_string).collect())
}

/// The lab's inputs found in `env`, as the JSON object Terraform's `inputs` variable takes.
pub fn inputs_json(spec: &Spec, env: &[(String, String)]) -> Option<String> {
    if spec.inputs.is_empty() {
        return None;
    }
    let map: serde_json::Map<String, serde_json::Value> =
        spec.inputs.iter().filter_map(|name| env.iter().find(|(k, _)| k == name).map(|(_, v)| (name.clone(), serde_json::Value::String(v.clone())))).collect();
    Some(serde_json::Value::Object(map).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    use std::path::PathBuf;

    #[test]
    fn a_newer_spec_asks_for_an_update() {
        assert!(load_error("unknown field `common`, expected one of `version`").starts_with("This lab needs a newer version of Cyber CTF"));
        assert!(load_error("invalid type: string, expected a map").starts_with("this lab's isoloom.yml:"));
    }

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: [CTF_LAUNCH_TOKEN]\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    inputs: [CTF_LAUNCH_TOKEN]\n    docker: { image: nginx:1.27 }\n";

    fn lab(spec: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-isoloom-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
        dir
    }

    #[test]
    fn generates_the_targets_files_into_the_lab() {
        let dir = lab(SPEC);
        prepare(&dir, Target::Docker).unwrap();
        let compose = std::fs::read_to_string(compose_file(&dir)).unwrap();
        assert!(compose.contains("isoloom.service.80"));
        // Named after the launcher's project for the lab, not the spec.
        let project = super::super::docker::project(dir.file_name().unwrap().to_str().unwrap());
        assert!(compose.contains(&format!("\nname: {project}\n")), "{compose}");
        prepare(&dir, Target::CloudDocker).unwrap();
        assert!(dir.join(".isoloom/cloud-docker/aws/main.tf").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explains_a_target_the_lab_cant_use() {
        let dir = lab(SPEC);
        // No `vm:` part: no VMs.
        let err = prepare(&dir, Target::Vagrant).unwrap_err().to_string();
        assert!(err.contains("can't run there"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_a_broken_spec_with_its_problems() {
        let dir = lab("version: 1\nname: t\nmachines: {}\nbogus: 1\n");
        assert!(spec(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_every_mistake_of_a_spec_that_parses() {
        let dir = lab("version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    vm: { os: debian-12 }\n");
        let err = spec(&dir).unwrap_err().to_string();
        assert!(err.starts_with("this lab's isoloom.yml has mistakes:\n"), "{err}");
        assert!(err.contains("machines.a.vm.provision"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn passes_only_declared_inputs() {
        let dir = lab(SPEC);
        let spec = spec(&dir).unwrap();
        let env = vec![("CTF_LAUNCH_TOKEN".to_string(), "t0k".to_string()), ("OTHER".to_string(), "x".to_string())];
        assert_eq!(inputs_json(&spec, &env).unwrap(), r#"{"CTF_LAUNCH_TOKEN":"t0k"}"#);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn destroys_an_old_deploy_run_with_its_own_module() {
        let dir = std::env::temp_dir().join(format!("cyberctf-legacy-{}", rand::random::<u32>()));
        std::fs::create_dir_all(dir.join("deploy/terraform/azure")).unwrap();
        assert_eq!(terraform_to_destroy(&dir, Runtime::Docker, "azure").unwrap(), dir.join("deploy/terraform/azure"));
        std::fs::create_dir_all(dir.join(".isoloom")).unwrap();
        assert_eq!(terraform_to_destroy(&dir, Runtime::Docker, "azure").unwrap(), dir.join(".isoloom/cloud-docker/azure"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vagrant_dir_and_target_differ_for_docker_and_vm() {
        let d = Path::new("/l");
        assert_eq!(vagrant_dir(d, Runtime::Docker), PathBuf::from("/l/.isoloom/docker-vm"));
        assert_eq!(vagrant_dir(d, Runtime::Vm), PathBuf::from("/l/.isoloom/vagrant"));
        assert_eq!(vagrant_target(Runtime::Docker), Target::DockerVm);
        assert_eq!(vagrant_target(Runtime::Vm), Target::Vagrant);
    }

    #[test]
    fn terraform_to_destroy_uses_isoloom_when_no_legacy_folder() {
        // A fresh lab with no deploy/terraform/<tf> falls through to the Isoloom module.
        let dir = std::env::temp_dir().join(format!("cyberctf-nolegacy-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(terraform_to_destroy(&dir, Runtime::Docker, "aws").unwrap(), dir.join(".isoloom/cloud-docker/aws"));
        // Even with .isoloom absent, a missing legacy folder still yields the Isoloom path.
        assert_eq!(terraform_to_destroy(&dir, Runtime::Vm, "proxmox").unwrap(), dir.join(".isoloom/proxmox"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn topology_and_message_come_from_the_resolved_spec() {
        let dir = lab(
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\n  back: { cidr: 10.31.0.0/24, internet: false }\nmachines:\n  web:\n    networks: { lab: 10, back: 10 }\n    services: [{ port: 80, http: true, publish: 8080 }]\n    docker: { image: nginx:1.27 }\n  db:\n    networks: { back: 20 }\n    docker: { image: redis:7 }\nmessage: |\n  Start at http://localhost:{{ machines.web.services.0.publish }}/ (web is {{ machines.web.addresses.lab }}).\n",
        );
        let spec = spec(&dir).unwrap();
        let (ifaces, networks) = topology(&spec);
        assert_eq!(
            networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str(), n.internal)).collect::<Vec<_>>(),
            [("lab", "10.30.0.0/24", false), ("back", "10.31.0.0/24", true)]
        );
        assert_eq!(ifaces["web"].iter().map(|i| (i.network.as_str(), i.ip.as_str())).collect::<Vec<_>>(), [("lab", "10.30.0.10"), ("back", "10.31.0.10")]);
        assert_eq!(ifaces["db"].len(), 1);
        assert_eq!(message(&dir, &spec, Target::Vagrant).as_deref(), Some("Start at http://localhost:8080/ (web is 10.30.0.10)."));
        // A UDP port of the same number is another service: it doesn't move the TCP one.
        assert_eq!(
            message_at(&dir, &spec, Target::Docker, &[("web".into(), 80, "udp".into(), 40053)]).as_deref(),
            Some("Start at http://localhost:8080/ (web is 10.30.0.10).")
        );
        // Run locally, the lab answers on the port picked at its first start: the message says so.
        assert_eq!(
            message_at(&dir, &spec, Target::Docker, &[("web".into(), 80, "tcp".into(), 46709)]).as_deref(),
            Some("Start at http://localhost:46709/ (web is 10.30.0.10).")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_instance_gets_its_own_files_and_blocks() {
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        let (a, b) = (labs.join("a"), labs.join("b"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("isoloom.yml"), SPEC).unwrap();
        }
        assert_eq!(ensure_instance(&labs, &a, &[], &[]).unwrap(), 1);
        assert_eq!(ensure_instance(&labs, &b, &[], &[]).unwrap(), 2);
        assert_eq!(ensure_instance(&labs, &a, &[], &[]).unwrap(), 1, "kept");
        let spec = prepare(&b, Target::Docker).unwrap();
        assert_eq!(spec.name, "t-2");
        assert_eq!(compose_file(&b), b.join(".isoloom-2/docker/compose.yml"));
        let compose = std::fs::read_to_string(compose_file(&b)).unwrap();
        assert!(compose.contains("10.32.0.0/24"), "the Docker block moved: {compose}");
        assert!(!b.join(".isoloom").exists());
        assert_eq!(vagrant_dir(&b, Runtime::Vm), b.join(".isoloom-2/vagrant"));
        // Its message gives the address its container really has (the moved block), not the spec's.
        let mut spec = spec;
        spec.message = Some("web is {{ machines.web.addresses.lab }}".into());
        assert_eq!(message(&b, &spec, Target::Docker).as_deref(), Some("web is 10.32.0.10"));
        assert_eq!(message(&b, &spec, Target::Vagrant).as_deref(), Some("web is 10.30.0.10"));
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn an_instance_skips_numbers_whose_blocks_docker_already_has() {
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        let a = labs.join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("isoloom.yml"), SPEC).unwrap();
        // SPEC is at 10.30.0.0/24: instance 1 would be 10.31, 2 is 10.32. Another program's
        // network covers 10.31 (a /16 here), so the lab gets 2.
        assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into(), "172.17.0.0/16".into()], &[]).unwrap(), 2);
        // Instance 2 is held by an environment the CLI runs elsewhere: 3 (10.33) then.
        std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
        assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into()], &[2]).unwrap(), 3);
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn a_stopped_labs_instance_moves_when_its_blocks_are_taken_now() {
        // Regression: the instance was picked once at install, with Docker down (no networks to
        // avoid), and kept for good: the start then failed on "Pool overlaps".
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        let a = labs.join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("isoloom.yml"), SPEC).unwrap();
        assert_eq!(ensure_instance(&labs, &a, &[], &[]).unwrap(), 1, "picked blind");
        // Docker now has a network on instance 1's block (10.31): the start moves the lab to 2.
        assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()], &[]).unwrap(), 2);
        assert_eq!(instance(&a), Some(2));
        // Clear: kept.
        assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()], &[]).unwrap(), 2);
        // Without a number yet, one is picked.
        std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
        assert_eq!(recheck_instance(&labs, &a, &[], &[]).unwrap(), 1);
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn labs_picking_their_instances_at_once_get_different_ones() {
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        let dirs: Vec<_> = (0..8).map(|i| labs.join(format!("lab{i}"))).collect();
        for d in &dirs {
            std::fs::create_dir_all(d).unwrap();
        }
        let picks: Vec<u8> = dirs
            .iter()
            .map(|d| {
                let (labs, d) = (labs.clone(), d.clone());
                std::thread::spawn(move || ensure_instance(&labs, &d, &[], &[]).unwrap())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        let mut unique = picks.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), picks.len(), "{picks:?}");
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn cidr_ranges() {
        assert_eq!(ipv4_range("10.31.0.0/24"), Some((u32::from(std::net::Ipv4Addr::new(10, 31, 0, 0)), u32::from(std::net::Ipv4Addr::new(10, 31, 0, 255)))));
        assert_eq!(ipv4_range("10.31.5.7/16").map(|r| r.0), Some(u32::from(std::net::Ipv4Addr::new(10, 31, 0, 0))));
        assert_eq!(ipv4_range("fd00::/64"), None);
    }

    #[test]
    fn observers_come_with_their_addresses() {
        let dir = lab(&format!("{SPEC}tools:\n  shell: {{}}\n"));
        let spec = spec(&dir).unwrap();
        let found = tools(&spec, true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "shell");
        assert_eq!(found[0].image.as_deref(), Some("nicolaka/netshoot"));
        assert_eq!(found[0].addresses.len(), 1);
        assert_eq!(found[0].addresses[0].network, "lab");
        assert!(tools(&spec, false)[0].addresses[0].ip.starts_with("10.30.0."));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_labs_own_defaults_file_reaches_the_generated_files() {
        let dir = lab(SPEC);
        std::fs::write(dir.join(isoloom_core::defaults::PROJECT_FILE), "cloud:\n  aws: { region: us-east-1 }\n").unwrap();
        prepare(&dir, Target::CloudDocker).unwrap();
        let tf = std::fs::read_to_string(dir.join(".isoloom/cloud-docker/aws/main.tf")).unwrap();
        assert!(tf.contains("us-east-1"), "{tf}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inputs_json_is_none_when_the_spec_declares_no_inputs() {
        let dir = lab(
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\n",
        );
        let spec = spec(&dir).unwrap();
        assert_eq!(inputs_json(&spec, &[("X".to_string(), "y".to_string())]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn requirements_read_isoloom_s_readiness() {
        use isoloom_core::host::Readiness;
        let r = |ready: bool, notes: &[&str]| {
            Some(Readiness { target: Target::CloudServices, cloud: Some("aws".into()), ready, notes: notes.iter().map(|n| n.to_string()).collect() })
        };
        assert_eq!(requirements_of(None), Requirements::None);
        assert_eq!(requirements_of(r(true, &["jq"])), Requirements::Met(vec!["jq".into()]));
        assert_eq!(requirements_of(r(false, &["jq", "! `gsed` isn't installed"])), Requirements::Missing(vec!["`gsed` isn't installed".into()]));
    }

    #[test]
    fn picks_the_module_for_each_run() {
        let d = Path::new("/l");
        assert_eq!(terraform(d, Runtime::Docker, "aws").unwrap(), (PathBuf::from("/l/.isoloom/cloud-docker/aws"), Target::CloudDocker));
        assert_eq!(terraform(d, Runtime::Docker, "proxmox").unwrap().0, PathBuf::from("/l/.isoloom/docker-vm/proxmox"));
        assert_eq!(terraform(d, Runtime::Vm, "proxmox").unwrap().1, Target::Proxmox);
        assert_eq!(terraform(d, Runtime::Vm, "aws").unwrap(), (PathBuf::from("/l/.isoloom/cloud-vm/aws"), Target::CloudVm));
        // Every cloud now has a cloud-vm module, not only AWS.
        assert_eq!(terraform(d, Runtime::Vm, "azure").unwrap(), (PathBuf::from("/l/.isoloom/cloud-vm/azure"), Target::CloudVm));
        assert_eq!(terraform(d, Runtime::Vm, "oci").unwrap(), (PathBuf::from("/l/.isoloom/cloud-vm/oci"), Target::CloudVm));
    }

    #[test]
    fn an_instance_marker_is_read_only_between_one_and_the_maximum() {
        let dir = lab(SPEC);
        for (text, want) in
            [("1", Some(1)), ("99", Some(99)), (" 7\r\n", Some(7)), ("0", None), ("100", None), ("", None), ("-1", None), ("x", None), ("1 2", None)]
        {
            std::fs::write(dir.join(INSTANCE_MARKER), text).unwrap();
            assert_eq!(instance(&dir), want, "{text:?}");
        }
        // A marker that is a folder, or none: no instance.
        std::fs::remove_file(dir.join(INSTANCE_MARKER)).unwrap();
        assert_eq!(instance(&dir), None);
        std::fs::create_dir(dir.join(INSTANCE_MARKER)).unwrap();
        assert_eq!(instance(&dir), None);
        assert_eq!(out(&dir), dir.join(".isoloom"), "no instance: the plain output folder");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_machine_with_every_instance_taken_refuses_a_new_lab() {
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-full-{}", rand::random::<u32>()));
        let a = labs.join("a");
        std::fs::create_dir_all(&a).unwrap();
        // No spec: every number is clear on the address side; the registry holds 1..=99.
        let elsewhere: Vec<u8> = (1..=isoloom_core::instance::MAX).collect();
        let err = ensure_instance(&labs, &a, &[], &elsewhere).unwrap_err().to_string();
        assert!(err.starts_with("no free instance for this lab"), "{err}");
        assert!(!a.join(INSTANCE_MARKER).exists(), "nothing written on failure");
        // One short of full: the last number is free.
        assert_eq!(ensure_instance(&labs, &a, &[], &elsewhere[..98]).unwrap(), 99);
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn a_staging_folder_left_by_a_crashed_install_still_holds_its_instance() {
        // The install copies the instance marker into `.<id>.staging` before the swap; a crash
        // between the two leaves that folder, and its number stays taken (never reused by
        // another lab, which would then share its blocks with the lab once the swap completes).
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-stage-{}", rand::random::<u32>()));
        let staging = labs.join(".a.staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join(INSTANCE_MARKER), "1").unwrap();
        // A stray file in the labs folder is not a lab.
        std::fs::write(labs.join("notes.txt"), "1").unwrap();
        let b = labs.join("b");
        std::fs::create_dir_all(&b).unwrap();
        assert_eq!(ensure_instance(&labs, &b, &[], &[]).unwrap(), 2);
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn cidr_ranges_at_the_prefix_boundaries() {
        let all = ipv4_range("0.0.0.0/0").unwrap();
        assert_eq!(all, (0, u32::MAX));
        let one = ipv4_range("10.1.2.3/32").unwrap();
        assert_eq!(one.0, one.1);
        assert_eq!(one.0, u32::from(std::net::Ipv4Addr::new(10, 1, 2, 3)));
        for bad in ["10.0.0.0/33", "10.0.0.0/-1", "10.0.0.0/", "/24", "10.0.0.0", "256.0.0.0/8", "10.0.0.0/24/8", " 10.0.0.0/24", "10.0.0.0/ 24"] {
            assert_eq!(ipv4_range(bad), None, "{bad}");
        }
    }

    #[test]
    fn spec_mistakes_are_reported_as_mistakes_not_crashes() {
        let base =
            "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    docker: { image: nginx:1.27 }\n";
        let cases: [(&str, &str, &str); 7] = [
            ("host out of the /24", &base.replace("lab: 10 }", "lab: 300 }"), "expected u8"),
            ("the network address itself", &base.replace("lab: 10 }", "lab: 0 }"), "isn't a usable address"),
            ("two machines at one address", &format!("{base}  db:\n    networks: {{ lab: 10 }}\n    docker: {{ image: redis:7 }}\n"), "already used by `web`"),
            ("a port past 65535", &base.replace("docker:", "services: [{ port: 70000, http: true }]\n    docker:"), "expected u16"),
            ("an IPv6 block", &base.replace("10.30.0.0/24", "'fd00::/64'"), "isn't a network address"),
            (
                "a machine declared twice",
                &format!("{base}  web:\n    networks: {{ lab: 11 }}\n    docker: {{ image: nginx:1.27 }}\n"),
                "duplicate entry with key \"web\"",
            ),
            ("a name that isn't kebab-case", &base.replace("name: t", "name: \"Lab é 1\""), "use kebab-case"),
        ];
        for (what, text, want) in cases {
            let dir = lab(text);
            let err = spec(&dir).unwrap_err().to_string();
            assert!(err.contains(want), "{what}: {err}");
            assert!(err.starts_with("this lab's isoloom.yml"), "{what}: {err}");
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn a_spec_with_windows_line_endings_reads_like_any_other() {
        let dir = lab(&SPEC.replace('\n', "\r\n"));
        let spec = spec(&dir).unwrap();
        assert_eq!(spec.name, "t");
        let (ifaces, networks) = topology(&spec);
        assert_eq!(networks[0].subnet, "10.30.0.0/24");
        assert_eq!(ifaces["web"][0].ip, "10.30.0.10");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_byte_order_mark_is_a_spec_error_not_a_crash() {
        // An editor on Windows may save the file with a BOM; the YAML parser then sees two
        // documents. The lab is refused with the parser's words (misleading, but not a crash).
        let dir = lab(&format!("\u{feff}{SPEC}"));
        let err = spec(&dir).unwrap_err().to_string();
        assert!(err.starts_with("this lab's isoloom.yml:"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_message_naming_a_value_the_lab_lacks_is_left_out() {
        let dir = lab(&format!("{SPEC}message: 'go to {{{{ machines.nope.addresses.lab }}}}'\n"));
        let spec = spec(&dir).unwrap();
        assert_eq!(message(&dir, &spec, Target::Docker), None);
        let mut spec = spec;
        spec.message = Some("http://localhost:{{ machines.web.services.0.publish }}/".into());
        assert_eq!(message_at(&dir, &spec, Target::Docker, &[("web".into(), 80, "tcp".into(), 40001)]).as_deref(), Some("http://localhost:40001/"));
        // Trailing blank lines of a block scalar are trimmed; inner ones kept.
        spec.message = Some("line one\n\nline two\n\n\n".into());
        assert_eq!(message(&dir, &spec, Target::Docker).as_deref(), Some("line one\n\nline two"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_message_with_an_unpublished_port_is_not_shown() {
        // SPEC's web service declares no `publish:`. On a target that publishes nothing (a VM
        // lab), or before any port was pinned, the placeholder names a value that is null, and
        // the player is shown a broken link. The doc of `message` promises None for a value the
        // spec doesn't have; `isoloom_core::resolved::fill` fills a null with "" instead.
        // Fails with: left: Some("http://localhost:/"), right: None.
        // Suggested fix: in `message_at`, treat a null (or empty) substitution as missing
        // (check the snapshot value before `fill`, or have isoloom-core's `fill` fail on null).
        let dir = lab(SPEC);
        let mut spec = spec(&dir).unwrap();
        spec.message = Some("http://localhost:{{ machines.web.services.0.publish }}/".into());
        assert_eq!(message(&dir, &spec, Target::Vagrant), None);
        assert_eq!(message_at(&dir, &spec, Target::Docker, &[]), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prepare_replaces_the_previous_files_but_keeps_other_ones_in_the_output_folder() {
        let dir = lab(SPEC);
        prepare(&dir, Target::Docker).unwrap();
        let compose = compose_file(&dir);
        std::fs::write(&compose, "stale").unwrap();
        // Something else in the output folder (a check runner's log, a pinned-ports file).
        std::fs::write(compose.with_file_name("notes.txt"), "mine").unwrap();
        prepare(&dir, Target::Docker).unwrap();
        assert!(std::fs::read_to_string(&compose).unwrap().contains("services"), "regenerated");
        assert_eq!(std::fs::read_to_string(compose.with_file_name("notes.txt")).unwrap(), "mine");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_instance_at_the_maximum_still_generates_and_resolves() {
        let dir = lab(SPEC);
        std::fs::write(dir.join(INSTANCE_MARKER), "99").unwrap();
        let spec = prepare(&dir, Target::Docker).unwrap();
        assert_eq!(spec.name, "t-99");
        assert!(compose_file(&dir).starts_with(dir.join(".isoloom-99")));
        assert!(compose_file(&dir).is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
