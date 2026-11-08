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

use std::path::Path;

use isothrum_runner::ShellCommand;

use super::attack_machine::{self, Host, Placed};
use super::launch::{VmStart, start_docker_in_vm_run, start_docker_run, start_vm_run};
use super::providers::Provider;
use super::real_labs::{check, install_copy, leftovers, log};
use super::{isothrum, lab, lifecycle};
use crate::platform::AppDirs;

/// `probe` run in the box, through the console the lab page opens, without a terminal.
async fn in_box(console: &ShellCommand, probe: &str) -> crate::error::Result<String> {
    let mut args = console.args.clone();
    match (console.program.as_str(), args.len()) {
        // `docker compose … exec <service> sh -c <shell>`: no TTY, the probe for the shell.
        ("docker", _) => {
            let exec = args.iter().position(|a| a == "exec").expect("an exec");
            args.insert(exec + 1, "-T".into());
            *args.last_mut().unwrap() = probe.into();
        }
        // `vagrant ssh <vm>`: the probe as its command.
        ("vagrant", 2) => args.extend(["-c".to_owned(), probe.to_owned()]),
        // `vagrant ssh <vm> -c <into the container>`: no TTY into it, the probe for its shell.
        ("vagrant", _) => {
            let into = args.last_mut().unwrap();
            let shell = into.find("sh -c '").expect("into a container's shell");
            *into = format!("{} sh -c '{probe}'", into[..shell].replace(" -it ", " -i "));
        }
        _ => panic!("an unexpected console: {console:?}"),
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let program = if console.program == "docker" { "docker" } else { "vagrant" };
    crate::platform::exec::run_env_timed(program, &args, console.cwd.as_deref(), &console.env, std::time::Duration::from_secs(120)).await
}

/// The box added to the running lab, reaching `target` from a shell in it, then taken out.
async fn exercise(placed: &Placed, dirs: &AppDirs, target: &str, l: &mut impl FnMut(String), f: &mut Vec<String>) {
    let added = placed.add(dirs, isoloom_manifest::access::DEFAULT_IMAGE, None, l).await;
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
    check(!attack_machine::added(&placed.dir), "the lab is as published".into(), f);
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
            lab::vagrant_target(super::Runtime::Docker),
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
