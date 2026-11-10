//! The attack box as a machine of the lab, on this machine, end to end, in each local form: a
//! lab started as the lab page starts it, the box added (the lab regenerated with it, the run
//! brought up to date), a lab machine reached from a shell in the box, the box taken out, the lab
//! stopped, nothing of it left. Opt-in (it needs Docker, or Vagrant and a hypervisor):
//!
//! ```text
//! CYBERCTF_TEST_BOX_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_BOX_FORM=docker|docker-vm|vm CYBERCTF_TEST_VM_PROVIDER=qemu \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 the_attack_box_is_a_machine_of_the_lab
//! ```
//!
//! `CYBERCTF_TEST_BOX_TARGET` names the lab machine to reach (default `web`, on port 80).
//!
//! The same in an AWS account, through the launcher's cloud start and stop, with the AWS CLI's
//! own sign-in (it is billed there while it runs, a few minutes; Kali's install on the VM form
//! takes about 20):
//!
//! ```text
//! CYBERCTF_TEST_BOX_LAB=<isoloom checkout>/examples/solo-web \
//! CYBERCTF_TEST_BOX_FORM=cloud-docker|cloud-vm CYBERCTF_TEST_AWS_REGION=eu-west-3 \
//! CYBERCTF_TEST_REAL_PUBLIC_IP=1 \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 the_attack_box_is_a_machine_of_an_aws_lab
//! ```

use std::path::Path;

use super::support::{add_box, box_reaches, check, in_box, install_copy, leftovers, log, remove_box, scenario};
use crate::platform::AppDirs;
use crate::runtime::attack_machine::{self, Placed};
use crate::runtime::host::Host;
use crate::runtime::launch::{CloudStart, Inputs, VmStart, start_cloud_run, start_docker_in_vm_run, start_docker_run, start_vm_run};
use crate::runtime::providers::Provider;
use crate::runtime::{isothrum, lab, lifecycle};

/// The box added to the running lab, reaching `target` from a shell in it, then taken out; with
/// the shared folder on (Settings, as [`shared_folder`] turns it on), that folder seen both ways
/// from inside the box.
pub(super) async fn exercise(placed: &Placed, dirs: &AppDirs, target: &str, l: &mut impl FnMut(String), f: &mut Vec<String>) {
    add_box(placed, dirs, attack_machine::default_image(), l, f).await;
    box_reaches(placed, target, "once added", f).await;
    match crate::shared_folder::current(dirs) {
        Ok(Some(folder)) => shared_both_ways(placed, &folder, f).await,
        Ok(None) => {}
        Err(e) => check(false, format!("the shared folder's setting reads: {e}"), f),
    }
    remove_box(placed, dirs, l, f).await;
    check(!attack_machine::added(&placed.dir).unwrap_or(true), "the lab is as published".into(), f);
}

/// The shared folder `folder` seen both ways from inside the box `placed`.
async fn shared_both_ways(placed: &Placed, folder: &Path, f: &mut Vec<String>) {
    let console = match placed.shell_command().await {
        Ok(console) => console,
        Err(e) => return check(false, format!("a console into the box opens: {e}"), f),
    };
    if let Err(e) = std::fs::write(folder.join("to-box"), "tools in") {
        return check(false, format!("a file is put in the shared folder: {e}"), f);
    }
    let mount = crate::shared_folder::MOUNT_POINT;
    let seen = in_box(&console, &format!("cat {mount}/to-box && echo loot out > {mount}/from-box")).await;
    check(seen.as_deref().is_ok_and(|o| o.contains("tools in")), format!("the box sees the shared folder at {mount} ({seen:?})"), f);
    let back = std::fs::read_to_string(folder.join("from-box")).unwrap_or_default();
    check(back.contains("loot out"), format!("what the box writes there lands on this computer ({back:?})"), f);
}

/// With `CYBERCTF_TEST_SHARED_FOLDER` (a folder; empty or unset: off), the shared folder turned on
/// there, as Settings do, the launcher's settings kept in `dirs`' data folder.
fn shared_folder(dirs: &AppDirs, f: &mut Vec<String>) {
    let Some(folder) = std::env::var("CYBERCTF_TEST_SHARED_FOLDER").ok().filter(|f| !f.is_empty()) else { return };
    if let Err(e) = dirs.data().map_err(|e| e.to_string()).and_then(|home| std::fs::create_dir_all(home).map_err(|e| e.to_string())) {
        return check(false, format!("the launcher's settings folder is made: {e}"), f);
    }
    let on = crate::shared_folder::shared_folder_set(dirs, true, &folder);
    check(on.is_ok(), format!("the shared folder is turned on at {folder} ({:?})", on.err()), f);
}

#[tokio::test]
#[ignore = "needs Docker, or Vagrant and a hypervisor, and a lab (CYBERCTF_TEST_BOX_LAB)"]
async fn the_attack_box_is_a_machine_of_the_lab_on_every_local_form() {
    let Ok(src) = std::env::var("CYBERCTF_TEST_BOX_LAB") else { return };
    let form = std::env::var("CYBERCTF_TEST_BOX_FORM").unwrap_or_else(|_| "docker".into());
    let target = std::env::var("CYBERCTF_TEST_BOX_TARGET").unwrap_or_else(|_| "web".into());
    let provider: Provider = serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "qemu".into()))).unwrap();
    let id = format!("box-{}", rand::random::<u16>());
    let dir = install_copy(Path::new(&src), &id);
    let home = std::env::temp_dir().join(format!("cyberctf-{id}-home"));
    // The app's data folder is the runner's home, as in the app: the shared folder's setting too.
    let dirs = AppDirs::new(Some(home.clone()), None);
    let (target_kind, vm) = match form.as_str() {
        "docker" => (lab::Target::Docker, false),
        "docker-vm" => (lab::vagrant_target(crate::runtime::Runtime::Docker), false),
        "vm" => (lab::Target::Vagrant, true),
        other => panic!("no form {other}"),
    };
    // The lab's run on this machine, in the form under test.
    let run = || match form.as_str() {
        "docker" => isothrum::LabRun::docker(&home, &dir, &id),
        "docker-vm" => isothrum::LabRun::docker_in_vm(&home, &dir, &id),
        _ => isothrum::LabRun::vm(&home, &dir, &id),
    };
    let mut f = Vec::new();
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let mut l = log(&id);
            let started = match form.as_str() {
                "docker" => start_docker_run(&home, &dir, &id, Inputs::default(), &mut l).await,
                "docker-vm" => start_docker_in_vm_run(&home, &dir, &id, VmStart { provider, inputs: Inputs::default() }, &mut l).await,
                _ => start_vm_run(&home, &dir, &id, VmStart { provider, inputs: Inputs::default() }, &mut l).await,
            };
            check(started.is_ok(), format!("the lab starts ({:?})", started.as_ref().err()), f);
            let placed = match run() {
                Ok(run) => Placed { host: Host::Local(run), dir: dir.clone(), target: target_kind, vm },
                Err(e) => return check(false, format!("the lab's run opens: {e}"), f),
            };
            if started.is_ok() {
                shared_folder(&dirs, f);
                exercise(&placed, &dirs, &target, &mut l, f).await;
            }
        },
        async |f: &mut Vec<String>| {
            let stopped = match run() {
                Ok(run) => lifecycle::stop_run(run, &dir, log(&id)).await,
                Err(e) => Err(e),
            };
            check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), f);
            check(leftovers(&id).await.is_empty(), "nothing of it is left on this machine's Docker".into(), f);
        },
    )
    .await;
    let _ = std::fs::remove_dir_all(&dir);
    assert!(f.is_empty(), "{f:#?}");
}

/// What of lab `name` AWS still holds in `region`: its instances not terminated, its networks, its
/// key pairs (Isoloom's names start `isoloom-<lab>-`).
async fn left_in_aws(region: &str, name: &str) -> String {
    let prefix = format!("isoloom-{name}-*");
    let queries: [&[&str]; 3] = [
        &[
            "ec2",
            "describe-instances",
            "--filters",
            "Name=instance-state-name,Values=pending,running,stopping,stopped",
            &format!("Name=tag:Name,Values={prefix}"),
            "--query",
            "Reservations[].Instances[].InstanceId",
        ],
        &["ec2", "describe-vpcs", "--filters", &format!("Name=tag:Name,Values={prefix}"), "--query", "Vpcs[].VpcId"],
        &["ec2", "describe-key-pairs", "--filters", &format!("Name=key-name,Values={prefix}"), "--query", "KeyPairs[].KeyName"],
    ];
    let mut left = String::new();
    for query in queries {
        let mut args = query.to_vec();
        args.extend(["--region", region, "--output", "text"]);
        left.push_str(crate::platform::exec::run_cli("aws", &args).await.unwrap_or_else(|e| format!("({e})")).trim());
    }
    left
}

#[tokio::test]
#[ignore = "needs an AWS account signed in with the AWS CLI, Terraform, and a lab (CYBERCTF_TEST_BOX_LAB); billed while it runs"]
async fn the_attack_box_is_a_machine_of_an_aws_lab() {
    use crate::testing::test_app::TestApp;
    let Ok(src) = std::env::var("CYBERCTF_TEST_BOX_LAB") else { return };
    let per_machine = std::env::var("CYBERCTF_TEST_BOX_FORM").is_ok_and(|f| f == "cloud-vm");
    let region = std::env::var("CYBERCTF_TEST_AWS_REGION").unwrap_or_else(|_| "eu-west-3".into());
    let target = std::env::var("CYBERCTF_TEST_BOX_TARGET").unwrap_or_else(|_| "web".into());
    let t = TestApp::new();
    let dirs = t.dirs();
    let host = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true, "host": region }));
    let id = format!("box-{}", rand::random::<u16>());
    let dir = t.lab(&id, &[]);
    lab::copy_tree(Path::new(&src), &dir).unwrap();
    let name = lab::load(&dir).unwrap().name;
    crate::runtime::server::mark_lab(&dir, Some(&host)).unwrap();
    let conn = crate::runtime::server::connection(&dirs, &host).unwrap();
    let mut f = Vec::new();
    scenario(
        &mut f,
        async |f: &mut Vec<String>| {
            let mut l = log(&id);
            let start = CloudStart { dir: &dir, id: &id, runtime: crate::runtime::Runtime::Docker, host: &host, conn, env: &[], per_machine };
            let started = start_cloud_run(&dirs, start, &mut l).await;
            check(started.is_ok(), format!("the lab starts ({:?})", started.as_ref().err()), f);
            if started.is_err() {
                return;
            }
            // The box's place, as the lab page's commands find it.
            match attack_machine::locate(&dirs, &dir, &id, crate::runtime::Runtime::Docker) {
                Ok(Some(placed)) => exercise(&placed, &dirs, &target, &mut l, f).await,
                found => check(false, format!("the box's place in the account is found ({:?})", found.err()), f),
            }
        },
        async |f: &mut Vec<String>| {
            let stopped = lifecycle::stop_locked(&dirs, &dir, &id, crate::runtime::Runtime::Docker, log(&id)).await;
            check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), f);
            let left = left_in_aws(&region, &name).await;
            check(left.is_empty(), format!("nothing of it is left in AWS ({left})"), f);
        },
    )
    .await;
    assert!(f.is_empty(), "{f:#?}");
}
