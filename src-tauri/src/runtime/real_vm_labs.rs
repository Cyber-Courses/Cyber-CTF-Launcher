//! A real VM lab through the launcher's runner path, end to end: generate the lab's files and
//! start it on a local hypervisor (`launch::start_vm_run`, the path the lab page takes with the
//! Isothrum runner chosen), read its status, run its checks, start the attack box beside it (the
//! lab's on-demand attack VM, hot-added by the runner), reach the lab's machine from a shell in
//! it and take it out, pause and resume the lab, shut it down and resume it, provision it again,
//! then stop it (`lifecycle::stop_vm_run`) and assert nothing of it is left. Opt-in (it needs
//! Vagrant, a hypervisor and its boxes):
//!
//! ```text
//! CYBERCTF_TEST_VM_LAB=<a VM lab: one Linux machine publishing an HTTP port, with checks> \
//! CYBERCTF_TEST_VM_PROVIDER=virtualbox \
//! cargo test real_vm_labs -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `CYBERCTF_TEST_VM_INPUTS` gives the launch's values (`NAME=value,…`); `CYBERCTF_TEST_ATTACK_BOX`
//! the attack VM's Vagrant box (default: the lab's own for this machine's CPU).

use std::path::{Path, PathBuf};

use isothrum_runner::ShellCommand;

use super::providers::Provider;
use super::real_labs::{check, detach_box, install_copy, log, vagrant_ssh, vm_inputs};
use super::{Park, Place, isothrum, launch, lifecycle};

/// One VM lab installed for the test.
struct Lab {
    dir: PathBuf,
    id: String,
    home: PathBuf,
    provider: Provider,
}

impl Lab {
    fn run(&self) -> isothrum::LabRun {
        isothrum::LabRun::vm(&self.home, &self.dir, &self.id).unwrap()
    }
}

/// Its status: running on a local VM, its machines up, and its entry answering here.
async fn up_and_answering(l: &Lab, f: &mut Vec<String>, when: &str) {
    match l.run().status().await {
        Ok(s) => {
            check(s.running && matches!(s.place, Some(Place::LocalVm)), format!("{when}: it runs on a local VM"), f);
            for m in s.machines.iter().filter(|m| !m.infra) {
                check(m.state == "running", format!("{when}: machine {} is running (state {})", m.name, m.state), f);
            }
            let Some(url) = s.url else { return check(false, format!("{when}: it has a local URL"), f) };
            let res = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().get(&url).send().await;
            check(res.as_ref().is_ok_and(|r| r.status().is_success()), format!("{when}: {url} answers ({:?})", res.map(|r| r.status())), f);
        }
        Err(e) => check(false, format!("{when}: its status reads: {e}"), f),
    }
}

/// Parks the lab in `mode`, checks it reads as parked, resumes it and checks it is back.
async fn park_and_resume(l: &Lab, mode: Park, f: &mut Vec<String>) {
    let parked = l.run().park(mode, &mut log(&l.id)).await;
    check(parked.is_ok(), format!("it parks ({mode:?}): {:?}", parked.err()), f);
    super::paths::mark_parked(&l.dir, Some(mode)).unwrap();
    let s = l.run().status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked == Some(mode)), format!("parked ({mode:?}), it reads as parked"), f);
    let resumed = l.run().resume(&mut log(&l.id)).await;
    check(resumed.is_ok(), format!("it resumes after {mode:?}: {:?}", resumed.err()), f);
    super::paths::mark_parked(&l.dir, None).unwrap();
    up_and_answering(l, f, &format!("resumed after {mode:?}")).await;
}

/// The attack box beside the lab: known down with the lab network to join, added to the running
/// lab as a VM (the runner's own, through Isoloom's hook, when the spec has no `access:` machine),
/// reported running at an address on the lab network, a shell into it that reaches the lab's
/// machine over that network, then taken out with the lab left up.
async fn attack_box(l: &Lab, f: &mut Vec<String>) {
    let run = l.run();
    let image = std::env::var("CYBERCTF_TEST_ATTACK_BOX").ok();
    let target = match run.status().await {
        Ok(s) => s.machines.iter().find(|m| !m.infra).map(|m| (m.name.clone(), m.ports.first().map_or(80, |p| p.target))),
        Err(_) => None,
    };
    match run.attack_status(None).await {
        Ok(s) => check(!s.running && !s.networks.is_empty(), format!("the box starts down, the lab network to join known ({:?})", s.networks), f),
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    if let Err(e) = run.attach(None, image.as_deref(), &mut log(&l.id)).await {
        return check(false, format!("the attack VM is hot-added: {e}"), f);
    }
    match run.attack_status(None).await {
        Ok(s) => {
            check(s.running, "the box reports running".into(), f);
            check(!s.ip.is_empty() && !s.lab_network.is_empty(), format!("it has an address on the lab network ({} on {})", s.ip, s.lab_network), f);
        }
        Err(e) => check(false, format!("the box's status reads: {e}"), f),
    }
    match (run.shell_command(), target) {
        (Ok(shell), Some((machine, port))) => shell_reaches(&shell, &machine, port, f).await,
        (Ok(_), None) => check(false, "the lab has a machine to reach".into(), f),
        (Err(e), _) => check(false, format!("the shell into the box is built: {e}"), f),
    }
    detach_box(&run, &l.id, f).await;
    up_and_answering(l, f, "with the attack VM gone").await;
}

/// The shell's command run non-interactively the same way (`vagrant ssh <define> -c …` with the
/// run's Vagrant environment): a request to `http://<machine>:<port>/` from inside the attack VM
/// over bash's `/dev/tcp` (nothing but a shell is assumed of the box), the machine known by name.
async fn shell_reaches(shell: &ShellCommand, machine: &str, port: u16, f: &mut Vec<String>) {
    let into_vm = shell.program == "vagrant" && shell.args.len() == 2 && shell.args[0] == "ssh";
    check(into_vm, format!("the shell is `vagrant ssh` into the attack VM ({} {:?})", shell.program, shell.args), f);
    let Some(define) = shell.args.get(1) else { return };
    let probe = format!("bash -c \"exec 3<>/dev/tcp/{machine}/{port} && printf 'GET / HTTP/1.0\\r\\nHost: {machine}\\r\\n\\r\\n' >&3 && head -c 300 <&3\"");
    let out = vagrant_ssh(shell, define, &probe).await;
    check(
        out.as_ref().is_ok_and(|o| o.contains("HTTP/1.") && o.contains(" 200 ")),
        format!("a shell in the attack VM reaches http://{machine}:{port}/ over the lab network ({out:?})"),
        f,
    );
}

/// The lab while it exists; every failure is collected so the lab is always stopped after.
async fn exercise(l: &Lab, f: &mut Vec<String>) {
    if let Err(e) = launch::start_vm_run(&l.home, &l.dir, &l.id, launch::VmStart { provider: l.provider, env: &vm_inputs() }, &mut log(&l.id)).await {
        return check(false, format!("it starts: {e}"), f);
    }
    up_and_answering(l, f, "started").await;
    match l.run().check().await {
        Ok(c) => check(c.available && c.ok, format!("its checks pass ({:?})", c.results.iter().map(|r| (&r.name, r.ok)).collect::<Vec<_>>()), f),
        Err(e) => check(false, format!("its checks run: {e}"), f),
    }
    attack_box(l, f).await;
    // vagrant-qemu has no `suspend`: a QEMU lab shuts down and resumes, never pauses.
    if l.provider != Provider::Qemu {
        park_and_resume(l, Park::Pause, f).await;
    }
    park_and_resume(l, Park::Shutdown, f).await;
    let provisioned = l.run().provision(None, &mut log(&l.id)).await;
    check(provisioned.is_ok(), format!("it provisions again: {:?}", provisioned.err()), f);
}

/// Whether the hypervisor still holds a VM of `lab` (by the names Isoloom gives them).
async fn left_on(provider: Provider, lab: &str) -> Vec<String> {
    let (program, args): (&str, &[&str]) = match provider {
        Provider::Virtualbox => ("VBoxManage", &["list", "vms"]),
        Provider::Libvirt => ("virsh", &["-c", "qemu:///system", "list", "--all", "--name"]),
        _ => return Vec::new(),
    };
    let out = crate::platform::exec::run(program, args, None).await.unwrap_or_default();
    out.lines().filter(|l| l.contains(&format!("{lab} · ")) || l.starts_with(&format!("{lab}_"))).map(str::to_owned).collect()
}

#[tokio::test]
#[ignore = "needs Vagrant, a hypervisor and a VM lab (CYBERCTF_TEST_VM_LAB)"]
async fn a_vm_lab_runs_end_to_end_through_the_runner() {
    let Ok(src) = std::env::var("CYBERCTF_TEST_VM_LAB") else { return };
    let provider: Provider =
        serde_json::from_value(serde_json::json!(std::env::var("CYBERCTF_TEST_VM_PROVIDER").unwrap_or_else(|_| "virtualbox".into()))).unwrap();
    let id = format!("vm-{}", rand::random::<u16>());
    let dir = install_copy(Path::new(&src), &id);
    let home = std::env::temp_dir().join(format!("cyberctf-runner-{id}"));
    let lab = Lab { dir, id, home, provider };
    let mut failures = Vec::new();
    exercise(&lab, &mut failures).await;
    let stopped = lifecycle::stop_vm_run(&lab.home, &lab.dir, &lab.id, log(&lab.id)).await;
    check(stopped.is_ok(), format!("it stops: {:?}", stopped.err()), &mut failures);
    let name = super::lab::lab(&lab.dir).map(|l| l.source().name.clone()).unwrap_or_default();
    let left = left_on(provider, &name).await;
    check(left.is_empty(), format!("nothing of it is left on {} ({left:?})", provider.id()), &mut failures);
    check(lab.run().status().await.is_ok_and(|s| !s.running && s.machines.is_empty()), "its status reads as gone".into(), &mut failures);
    let _ = std::fs::remove_dir_all(&lab.dir);
    let _ = std::fs::remove_dir_all(&lab.home);
    assert!(failures.is_empty(), "{failures:#?}");
}
