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

use super::support::{check, in_box, install_copy, leftovers, log};
use crate::platform::AppDirs;
use crate::runtime::attack_machine::{self, Host, Placed};
use crate::runtime::launch::{CloudStart, VmStart, start_cloud_run, start_docker_in_vm_run, start_docker_run, start_vm_run};
use crate::runtime::providers::Provider;
use crate::runtime::{isothrum, lab, lifecycle};

/// The box added to the running lab, reaching `target` from a shell in it, then taken out.
pub(super) async fn exercise(placed: &Placed, dirs: &AppDirs, target: &str, l: &mut impl FnMut(String), f: &mut Vec<String>) {
    let added = placed.add(dirs, attack_machine::default_image(), None, l).await;
    check(added.is_ok(), format!("the box is added ({:?})", added.err()), f);
    match placed.status(None).await {
        Ok(s) => check(s.running && !s.ip.is_empty(), format!("the box runs, with an address ({s:?})"), f),
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    let console = placed.shell_command().expect("a console into the box");
    let probe = format!("bash -c \"exec 3<>/dev/tcp/{target}/80 && echo reached; hostname\"");
    let reached = in_box(&console, &probe).await;
    check(
        reached.as_deref().is_ok_and(|o| o.contains("reached") && o.contains(attack_machine::NAME)),
        format!("the box reaches {target} and is called {} ({reached:?})", attack_machine::NAME),
        f,
    );
    let removed = placed.remove(dirs, l).await;
    check(removed.is_ok(), format!("the box is taken out ({:?})", removed.err()), f);
    match placed.status(None).await {
        Ok(s) => check(!s.running, "the box is gone".into(), f),
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    check(!attack_machine::added(&placed.dir).unwrap_or(true), "the lab is as published".into(), f);
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
    let dirs = AppDirs::new(None, None);
    let mut f = Vec::new();
    let mut l = log(&id);
    let (started, run, target_kind, vm) = match form.as_str() {
        "docker" => {
            (start_docker_run(&home, &dir, &id, &[], &mut l).await, isothrum::LabRun::docker(&home, &dir, &id).unwrap(), isoloom_spec::Target::Docker, false)
        }
        "docker-vm" => (
            start_docker_in_vm_run(&home, &dir, &id, VmStart { provider, env: &[] }, &mut l).await,
            isothrum::LabRun::docker_in_vm(&home, &dir, &id).unwrap(),
            lab::vagrant_target(crate::runtime::Runtime::Docker),
            false,
        ),
        "vm" => (
            start_vm_run(&home, &dir, &id, VmStart { provider, env: &[] }, &mut l).await,
            isothrum::LabRun::vm(&home, &dir, &id).unwrap(),
            isoloom_spec::Target::Vagrant,
            true,
        ),
        other => panic!("no form {other}"),
    };
    check(started.is_ok(), format!("the lab starts ({:?})", started.as_ref().err()), &mut f);
    let placed = Placed { host: Host::Local(run), dir: dir.clone(), target: target_kind, vm };
    if started.is_ok() {
        exercise(&placed, &dirs, &target, &mut l, &mut f).await;
    }
    let stopped = match form.as_str() {
        "docker" => lifecycle::stop_docker_run(&home, &dir, &id, log(&id)).await,
        "docker-vm" => lifecycle::stop_docker_in_vm_run(&home, &dir, &id, log(&id)).await,
        _ => lifecycle::stop_vm_run(&home, &dir, &id, log(&id)).await,
    };
    check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), &mut f);
    check(leftovers(&id).await.is_empty(), "nothing of it is left on this machine's Docker".into(), &mut f);
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
    let (region_of, tf_env) = (conn.region.clone(), conn.tf_env.clone());
    let mut f = Vec::new();
    let mut l = log(&id);
    let start = CloudStart { dir: &dir, id: &id, runtime: crate::runtime::Runtime::Docker, host: &host, conn, env: &[], per_machine };
    let started = start_cloud_run(&dirs, start, &mut l).await;
    check(started.is_ok(), format!("the lab starts ({:?})", started.as_ref().err()), &mut f);
    if started.is_ok() {
        // The box's place, as the lab page's commands make it.
        let form = crate::runtime::paths::local_runtime(&dir, crate::runtime::Runtime::Docker);
        let place = isothrum::CloudPlace { label: host.clone(), region: region_of.clone(), tf_env: tf_env.clone(), ..isothrum::CloudPlace::default() };
        let run = isothrum::CloudRun::new(&crate::runtime::paths::runner_home(&dirs).unwrap(), &dir, &id, form, "aws").unwrap();
        let target_kind = lab::terraform(&dir, form, "aws").1;
        let placed = Placed { host: Host::Cloud(run, place), dir: dir.clone(), target: target_kind, vm: form == crate::runtime::Runtime::Vm };
        exercise(&placed, &dirs, &target, &mut l, &mut f).await;
    }
    let stopped = lifecycle::stop_locked(&dirs, &dir, &id, crate::runtime::Runtime::Docker, log(&id)).await;
    check(stopped.is_ok(), format!("the lab stops ({:?})", stopped.err()), &mut f);
    let left = left_in_aws(&region, &name).await;
    check(left.is_empty(), format!("nothing of it is left in AWS ({left})"), &mut f);
    assert!(f.is_empty(), "{f:#?}");
}
