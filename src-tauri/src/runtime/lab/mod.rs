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
//! This module is the launcher's one door to Isoloom (its crates are used nowhere else): the
//! spec, the generated files, the resolved snapshot and the checks go through it, and so do
//! Isoloom's path and naming conventions (`names`). The launcher's own conventions sit on top:
//! its overrides of the generated files (`overrides`, applied after Isoloom's defaults) and the
//! variables it gives Isoloom's Terraform modules from its hosts and cloud accounts (`vars`).
//! Isoloom only converts: it keeps no registry of running labs and probes no host, so neither
//! does this module.

use std::collections::HashMap;
use std::path::Path;

use super::{Interface, Network};
use crate::error::{Error, Result};

mod instances;
mod names;
mod overrides;
mod vars;

pub use instances::*;
pub use names::*;
pub use vars::{esxi_env, host_vars};

// What the rest of the launcher uses of Isoloom's types: through here, like its functions.
pub use isoloom_spec::model::{Arch, Spec, Target};
pub use isoloom_spec::model::{CloudProvider, CloudServices};
pub use isoloom_targets::Lab;

/// The lab in `dir` as its author wrote it, checked (its fields, its lowered form's and the files
/// it names), with this machine's Isoloom defaults' image table.
pub fn lab(dir: &Path) -> Result<Lab> {
    let source = isoloom_spec::load(dir).map_err(|e| Error::Invalid(load_error(&e)))?;
    let lab = Lab::new(source).with_images(defaults(dir)?.images);
    let problems: Vec<String> = lab.problems_with_files(dir).iter().map(|p| p.render(isoloom_spec::SPEC_FILE)).collect();
    if !problems.is_empty() {
        return Err(Error::Invalid(format!("this lab's isoloom.yml has mistakes:\n{}", problems.join("\n"))));
    }
    Ok(lab)
}

/// A spec as written, unchecked (what a launch dialog previews): `isoloom.yml` in `dir`.
pub fn load(dir: &Path) -> Result<Spec> {
    isoloom_spec::load(dir).map(|s| Lab::new(s).spec()).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// A spec from its text, unchecked.
pub fn parse(text: &str) -> Result<Spec> {
    isoloom_spec::parse(text).map(|s| Lab::new(s).spec()).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// Copies a lab checkout `src` into `dst`, leaving out generated output and VCS/VM state
/// (`.isoloom`, `.git`, `.vagrant`). Used to install a lab imported from a local directory, and in
/// tests to stage a corpus lab. Copied in Rust, not with `cp -R`, so it runs on Windows too.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if [".isoloom", ".git", ".vagrant"].iter().any(|skip| name == **skip) {
            continue;
        }
        let (from, to) = (entry.path(), dst.join(&name));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            // Skip symlinks, as the GitHub-tarball install does (its extract takes only files and
            // dirs): a published lab never relies on one, and following it could fail the copy
            // (a dangling link) or escape the tree.
            continue;
        }
        if kind.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// A spec this build can't read. A key the format doesn't have means the lab uses a newer format
/// than this launcher (labs adopt spec additions before every player has updated), so say what
/// to do instead of showing only Isoloom's problems. A missing or unreadable file, or a mistake
/// of the lab's own, is shown as Isoloom words it.
fn load_error(e: &isoloom_spec::Error) -> String {
    let newer = matches!(e, isoloom_spec::Error::Invalid { problems, .. } if problems.iter().any(|p| p.code == isoloom_spec::syntax::Code::UnknownField));
    if newer {
        format!("This lab needs a newer version of Cyber CTF. Update it (Settings > About), then start the lab again.\n(isoloom.yml: {e})")
    } else {
        format!("this lab's isoloom.yml: {e}")
    }
}

/// This machine's Isoloom defaults for the lab: the user's `~/.isoloom/defaults.yml`, the
/// lab's own `isoloom.defaults.yml`, then `ISOLOOM_*` in the environment (the same hierarchy
/// the `isoloom` CLI reads), so a user's image table or cloud regions hold here too.
fn defaults(dir: &Path) -> Result<isoloom_spec::defaults::Defaults> {
    let sources = isoloom_spec::defaults::Sources { project: dir, images_file: None, profile: None, sets: &[], host: host() };
    isoloom_spec::defaults::load(&sources).map(|r| r.defaults).map_err(|e| Error::Invalid(format!("this machine's Isoloom defaults: {e}")))
}

/// The files [`spec`] reads for the lab in `dir`: its spec (either name Isoloom accepts), its
/// own defaults, its instance marker, and the user's Isoloom defaults (wherever Isoloom finds
/// them). What a cache of a spec watches.
pub fn spec_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let own = isoloom_spec::SPEC_FILES.iter().copied().chain(["isoloom.defaults.yml", INSTANCE_MARKER]).map(|f| dir.join(f));
    own.chain([isoloom_spec::defaults::user_file(&host())]).collect()
}

/// This machine as Isoloom reads it: its home and the `ISOLOOM_*` variables.
fn host() -> isoloom_spec::host::Host {
    isoloom_spec::host::Host::new(std::env::vars_os())
}

/// `spec` as instance `n` of it (unchanged without one).
fn as_instance(spec: Spec, n: Option<u8>) -> Result<Spec> {
    match n {
        Some(n) => isoloom_spec::instance::apply(&spec, n).map_err(|e| Error::Invalid(format!("this lab as instance {n}: {e}"))),
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

/// The observers of the lab in `dir` (as its instance), at their addresses: the container ones
/// when the lab runs as containers (`docker`), else the ones the spec writes.
pub fn tools(dir: &Path, docker: bool) -> Result<Vec<Observer>> {
    let snapshot = isoloom_targets::resolved::resolve_with(&lab(dir)?, instance(dir));
    let key = if docker { "docker_addresses" } else { "addresses" };
    Ok(snapshot["tools"]
        .as_object()
        .map(|tools| {
            tools
                .iter()
                .map(|(name, t)| Observer {
                    name: name.clone(),
                    image: t["image"].as_str().map(str::to_string),
                    addresses: interfaces(&t[key]),
                    publish: t["publish"].as_u64().and_then(|p| u16::try_from(p).ok()),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// A snapshot's `{ network: address }` as interfaces.
fn interfaces(addresses: &serde_json::Value) -> Vec<Interface> {
    addresses
        .as_object()
        .map(|a| a.iter().filter_map(|(net, ip)| ip.as_str().map(|ip| Interface { network: net.clone(), ip: ip.to_string() })).collect())
        .unwrap_or_default()
}

/// A lab ready to run on one target: its spec as written (what Isoloom resolves its message and
/// refusals from) and as the target runs it, this lab's instance of it.
#[derive(Debug)]
pub struct Prepared {
    /// The lab as its author wrote it (boxed: an authored spec is large, and a start holds it
    /// across its awaits).
    pub lab: Box<Lab>,
    /// The spec as the target runs it.
    pub spec: Spec,
}

/// Generates `target`'s files into the lab (overwriting the previous ones), and returns the lab
/// ready to run there. Fails with Isoloom's own reason when the lab can't run there (e.g.
/// Windows machines on a target without Windows images). Isoloom's defaults are applied to the
/// files, then the launcher's own overrides (see `overrides`).
pub fn prepare(dir: &Path, target: Target) -> Result<Prepared> {
    let lab = lab(dir)?;
    let n = instance(dir);
    let files = generate(&lab, target, n).map_err(|e| Error::Invalid(format!("this lab can't run there: {e}")))?;
    let files = isoloom_spec::defaults::apply_to_files(files, &defaults(dir)?);
    for f in overrides::apply(files, &launcher_overrides(dir, target)) {
        let path = dir.join(f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, f.contents)?;
    }
    let spec = as_instance(lab.for_target(target), n)?;
    Ok(Prepared { lab: Box::new(lab), spec })
}

/// `target`'s files for `lab`, as Isoloom generates them: the plan rendered by the target's
/// backend, its run manifest (`<target>/run.json`) when it has one, and the resolved snapshot;
/// for instance `n`, the lab as that instance and everything under `.isoloom-<n>/`. Refused,
/// with the backend's reason, when the lab's machines don't allow the target or the plan needs
/// what the backend can't do.
fn generate(lab: &Lab, target: Target, n: Option<u8>) -> isoloom_targets::Result<Vec<isoloom_targets::GeneratedFile>> {
    match n {
        Some(n) => isoloom_targets::generate_instance(lab, target, n),
        None => isoloom_targets::generate(lab, target),
    }
}

/// The launcher's overrides of a lab's files for `target`: on local Docker, its Compose project
/// (`cyberctf-<lab id>`, the lab's folder being its id).
fn launcher_overrides(dir: &Path, target: Target) -> overrides::Overrides {
    let id = dir.file_name().and_then(|n| n.to_str()).filter(|_| target == Target::Docker);
    overrides::Overrides { compose_project: id.map(super::docker::project) }
}

/// Each machine's lab interfaces and the lab's networks, from Isoloom's resolved snapshot of
/// the lab: addresses are static (declared in the lab), so the diagram of a VM lab comes from
/// here, without reaching into the guests or reading the generated files.
pub fn topology(lab: &Lab) -> (HashMap<String, Vec<Interface>>, Vec<Network>) {
    let snapshot = isoloom_targets::resolved::resolve(lab);
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
    let ifaces = snapshot["machines"]
        .as_object()
        .map(|machines| machines.iter().map(|(name, m)| (name.clone(), interfaces(&m["addresses"]))).collect())
        .unwrap_or_default();
    (ifaces, networks)
}

/// The memory the lab's machines ask for, in MB.
pub fn memory_mb(spec: &Spec) -> u32 {
    isoloom_spec::totals(spec).memory_mb
}

/// Why the lab's VMs can't run on Vagrant's QEMU provider (its networks), if they can't.
pub fn qemu_refusal(lab: &Lab) -> Option<String> {
    isoloom_targets::refusal_on(lab, Target::Vagrant, Some("qemu"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    use std::path::PathBuf;

    const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: [CTF_LAUNCH_TOKEN]\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    files: [{ path: /run/lab/token, input: CTF_LAUNCH_TOKEN }]\n    app: { image: nginx:1.27 }\n";

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
        assert!(compose.contains("isoloom.port.web"), "{compose}");
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
        // An image's filesystem can't be reproduced on a fresh VM: no VMs.
        let err = prepare(&dir, Target::Vagrant).unwrap_err().to_string();
        assert!(err.starts_with("this lab can't run there: vagrant") && err.contains("machines.web.app.image"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_backend_refusal_keeps_its_reason() {
        // Every target either renders (with the snapshot) or refuses in Isoloom's words, named
        // after the target: what the player reads after "can't run there".
        let dir = lab(SPEC);
        let lab = super::lab(&dir).unwrap();
        for target in Target::ALL.iter().filter(|t| isoloom_targets::backend(**t).is_some()) {
            match generate(&lab, *target, None) {
                Ok(files) => assert!(files.iter().any(|f| f.path == isoloom_targets::resolved::PATH), "{target:?}: the snapshot goes with every target"),
                Err(e) => assert!(e.to_string().starts_with(target.id()), "{target:?}: {e}"),
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_snapshot_read_here_is_version_three() {
        // `tools`, `topology` and `blocks_clear` read the resolved snapshot's v3 fields
        // (`networks.*.cidr`, `docker_cidr`, `internet`, `machines.*.addresses`, `tools`). A new
        // version changes or drops fields: this fails so they are read again.
        assert_eq!(isoloom_targets::resolved::RESOLVED_VERSION, 3);
        let dir = lab(SPEC);
        assert_eq!(isoloom_targets::resolved::resolve(&super::lab(&dir).unwrap())["resolved_version"], 3);
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
    fn an_instance_skips_numbers_whose_blocks_docker_already_has() {
        let labs = std::env::temp_dir().join(format!("cyberctf-labs-{}", rand::random::<u32>()));
        let a = labs.join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("isoloom.yml"), SPEC).unwrap();
        // SPEC is at 10.30.0.0/24: instance 1 would be 10.31, 2 is 10.32. Another program's
        // network covers 10.31 (a /16 here), so the lab gets 2.
        assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into(), "172.17.0.0/16".into()]).unwrap(), 2);
        // Instance 2 is held by another installed lab: 3 (10.33) then.
        std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
        std::fs::create_dir_all(labs.join("b")).unwrap();
        std::fs::write(labs.join("b").join(INSTANCE_MARKER), "2").unwrap();
        assert_eq!(ensure_instance(&labs, &a, &["10.31.0.0/16".into()]).unwrap(), 3);
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
        assert_eq!(ensure_instance(&labs, &a, &[]).unwrap(), 1, "picked blind");
        // Docker now has a network on instance 1's block (10.31): the start moves the lab to 2.
        assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()]).unwrap(), 2);
        assert_eq!(instance(&a), Some(2));
        // Clear: kept.
        assert_eq!(recheck_instance(&labs, &a, &["10.31.0.0/24".into()]).unwrap(), 2);
        // Without a number yet, one is picked.
        std::fs::remove_file(a.join(INSTANCE_MARKER)).unwrap();
        assert_eq!(recheck_instance(&labs, &a, &[]).unwrap(), 1);
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
                std::thread::spawn(move || ensure_instance(&labs, &d, &[]).unwrap())
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
        let found = tools(&dir, true).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "shell");
        assert_eq!(found[0].image.as_deref(), Some("nicolaka/netshoot"));
        assert_eq!(found[0].addresses.len(), 1);
        assert_eq!(found[0].addresses[0].network, "lab");
        assert!(tools(&dir, false).unwrap()[0].addresses[0].ip.starts_with("10.30.0."));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_labs_own_defaults_file_reaches_the_generated_files() {
        let dir = lab(SPEC);
        std::fs::write(dir.join(isoloom_spec::defaults::PROJECT_FILE), "cloud:\n  aws: { region: us-east-1 }\n").unwrap();
        prepare(&dir, Target::CloudDocker).unwrap();
        let tf = std::fs::read_to_string(dir.join(".isoloom/cloud-docker/aws/main.tf")).unwrap();
        assert!(tf.contains("us-east-1"), "{tf}");
        std::fs::remove_dir_all(dir).unwrap();
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
        // No spec: every number is clear on the address side; other labs hold 1..=99.
        for n in 1..=isoloom_spec::instance::MAX {
            let other = labs.join(format!("other{n}"));
            std::fs::create_dir_all(&other).unwrap();
            std::fs::write(other.join(INSTANCE_MARKER), n.to_string()).unwrap();
        }
        let err = ensure_instance(&labs, &a, &[]).unwrap_err().to_string();
        assert!(err.starts_with("no free instance for this lab"), "{err}");
        assert!(!a.join(INSTANCE_MARKER).exists(), "nothing written on failure");
        // One short of full: the last number is free.
        std::fs::remove_dir_all(labs.join("other99")).unwrap();
        assert_eq!(ensure_instance(&labs, &a, &[]).unwrap(), 99);
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
        assert_eq!(ensure_instance(&labs, &b, &[]).unwrap(), 2);
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
    fn a_spec_with_windows_line_endings_reads_like_any_other() {
        let dir = lab(&SPEC.replace('\n', "\r\n"));
        let lab = super::lab(&dir).unwrap();
        assert_eq!(lab.spec().name, "t");
        let (ifaces, networks) = topology(&lab);
        assert_eq!(networks[0].subnet, "10.30.0.0/24");
        assert_eq!(ifaces["web"][0].ip, "10.30.0.10");
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
        let prepared = prepare(&dir, Target::Docker).unwrap();
        assert_eq!(prepared.spec.name, "t-99");
        assert!(compose_file(&dir).starts_with(dir.join(".isoloom-99")));
        assert!(compose_file(&dir).is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
