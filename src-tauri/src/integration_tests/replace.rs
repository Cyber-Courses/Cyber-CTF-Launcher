//! A running lab's folder is never replaced under it, on each local form: started through the
//! launcher's own start, a new version installed over it and the same lab imported again are both
//! refused while it is up (the runner's record says so, wherever the runner keeps it), and both go
//! through once it is stopped. Opt-in (it needs Docker, and for the VM forms Vagrant and a
//! hypervisor):
//!
//! ```text
//! CYBERCTF_TEST_REPLACE_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_REPLACE_FORM=docker-vm \
//! CYBERCTF_TEST_VM_PROVIDER=utm \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 a_running_lab_is_not_replaced_under_it
//! ```
//!
//! `CYBERCTF_TEST_REPLACE_FORM` is `docker`, `docker-vm` or `vm`; `CYBERCTF_TEST_VM_INPUTS` gives
//! a VM lab's launch values (`NAME=value,…`).

use std::path::Path;

use super::support::lab::vm_inputs;
use super::support::{check, log, scenario};
use crate::error::Code;
use crate::runtime::providers::Provider;
use crate::runtime::{Destination, LabRef, Runtime, lab, stop_lab};
use crate::testing::test_app::TestApp;

/// A newer commit than the one the copied lab has (none).
const NEWER: &str = "1123456789abcdef0123456789abcdef01234567";

/// The lab at `src` as GitHub's tarball of it: its files under one top folder.
fn tarball(src: &Path) -> std::io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    builder.append_dir_all("lab-newer", src)?;
    builder.into_inner()?.finish()
}

/// Whether `result` is the refusal of a lab still up on this machine.
fn refused_here<T>(result: &crate::error::Result<T>) -> bool {
    result.as_ref().err().is_some_and(|e| e.is_refusal() && e.code() == Code::LabRunningHere)
}

#[tokio::test]
#[ignore = "needs Docker (and Vagrant with a hypervisor for the VM forms) and a lab (CYBERCTF_TEST_REPLACE_LAB)"]
async fn a_running_lab_is_not_replaced_under_it() {
    let Some(src) = super::support::knob("CYBERCTF_TEST_REPLACE_LAB") else { return };
    let form = std::env::var("CYBERCTF_TEST_REPLACE_FORM").unwrap_or_else(|_| "docker".into());
    let provider: Provider =
        serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "virtualbox".into()))).unwrap();
    let (runtime, provider, env) = match form.as_str() {
        "docker" => (Runtime::Docker, None, Vec::new()),
        "docker-vm" => (Runtime::Docker, Some(provider), Vec::new()),
        "vm" => (Runtime::Vm, Some(provider), vm_inputs()),
        other => panic!("CYBERCTF_TEST_REPLACE_FORM: {other} is none of docker, docker-vm, vm"),
    };
    let src = Path::new(&src);
    let newer = tarball(src).unwrap_or_else(|e| panic!("the lab's tarball: {e}"));
    let t = TestApp::new();
    let id = format!("replace-{}", rand::random::<u16>());
    // Installed as the launcher installs a lab: under the app data folder.
    let dir = t.data().join("labs").join(&id);
    lab::copy_tree(src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    let on_vm = match form.as_str() {
        "docker-vm" => Some(lab::Target::DockerVm),
        "vm" => Some(lab::Target::Vagrant),
        _ => None,
    };
    if let (Some(target), Some(provider)) = (on_vm, provider)
        && super::support::refused_by_launcher(&dir, target, provider)
    {
        return;
    }
    let started = std::time::Instant::now();
    let mut f = Vec::new();
    let to = Destination { provider, ..Destination::default() };
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let up = crate::runtime::start(&t.dirs(), LabRef { dir: &dir, id: &id, runtime }, to, &env, log(&id)).await;
            check(up.is_ok(), format!("it starts ({form}): {:?}", up.as_ref().err()), f);
            if up.is_err() {
                return;
            }
            let probe = std::fs::write(dir.join(".replace-probe"), "the running copy");
            check(probe.is_ok(), format!("a file is left in the running copy's folder ({:?})", probe.err()), f);
            let update = crate::labs::for_tests::update(&t.dirs(), &id, runtime, NEWER, newer.clone()).await;
            check(refused_here(&update), format!("a new version is refused while it runs ({:?})", update.err()), f);
            let reimport = crate::labs::for_tests::reimport(&t.dirs(), &id, src).await;
            check(refused_here(&reimport), format!("an import of it again is refused while it runs ({:?})", reimport.err()), f);
            check(dir.join(".replace-probe").is_file(), "the running copy's folder is untouched".into(), f);
        },
        async |f: &mut Vec<String>| {
            let stopped = stop_lab(&t.dirs(), &id, runtime, std::future::ready(()), log(&id)).await;
            check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
        },
    )
    .await;
    let reimport = crate::labs::for_tests::reimport(&t.dirs(), &id, src).await;
    check(reimport.is_ok(), format!("stopped, it imports again ({:?})", reimport.err()), &mut f);
    let update = crate::labs::for_tests::update(&t.dirs(), &id, runtime, NEWER, newer).await;
    check(update.is_ok(), format!("stopped, the new version installs ({:?})", update.err()), &mut f);
    check(!dir.join(".replace-probe").exists(), "the folder was replaced".into(), &mut f);
    println!("replace ({form}): {:.0?}", started.elapsed());
    assert!(f.is_empty(), "replacing a running lab ({form}) failed:\n{}", f.join("\n"));
}
