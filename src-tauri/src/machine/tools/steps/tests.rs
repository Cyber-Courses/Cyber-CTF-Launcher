use isothrum_machine::install::step;

use super::*;

#[tokio::test]
async fn what_a_check_read_while_a_step_ran_is_forgotten_once_it_ends() {
    // Regression: the kept answers were forgotten only before the step, so a machine check
    // during a `vagrant plugin install` kept the plugin list without the plugin, and the
    // finished install still read as "needs setup" for minutes.
    use crate::platform::remembered::{is_kept, keep};
    let key = format!("vagrant plugin list {}", rand::random::<u32>());
    let during = key.clone();
    let _tools = crate::platform::exec::fake::Tools::new(move |_, _| {
        keep(&during, "vagrant-qemu (0.8.0, global)");
        crate::platform::exec::fake::ok("Installed the plugin 'vagrant-vmware-desktop (3.0.5)'!")
    });
    run_step(&step("vagrant", &["plugin", "install", "vagrant-vmware-desktop"]), &mut |_| {}).await.unwrap();
    assert!(!is_kept(&key), "what the check read during the step is forgotten");
}

#[tokio::test]
async fn a_plan_streams_its_lines_says_its_notes_and_stops_at_its_first_failure() {
    let _tools = crate::platform::exec::fake::Tools::new(|program, _| match program {
        "first" => crate::platform::exec::fake::ok("out-1\nout-2"),
        _ => crate::platform::exec::fake::fail("broken"),
    });
    let mut noted = step("first", &["a"]);
    noted.note = Some(Note::QemuUpdatingHomebrewFirst);
    let mut lines = Vec::new();
    let err = run_plan(&[noted, step("second", &[]), step("third", &[])], &mut |l| lines.push(l)).await.unwrap_err();
    assert_eq!(lines, [said(Note::QemuUpdatingHomebrewFirst).as_str(), "out-1", "out-2", "$ second ", "broken"]);
    assert_eq!(err.to_string(), "`second` exited with exit status: 1");
}

#[test]
fn isothrum_machines_step_failures_read_as_the_launcher_said_them() {
    use isothrum_machine::install::brew::Trouble;
    assert_eq!(step_failed(StepFailed::NoPkexec, false).to_string(), NO_PKEXEC);
    assert!(step_failed(StepFailed::PromptRefused, false).to_string().contains("cancelled"));
    assert!(step_failed(StepFailed::NoPolkitAgent, false).to_string().contains("polkit agent"));
    assert_eq!(step_failed(StepFailed::SudoRefused, false).to_string(), SUDO_REFUSED);
    // A script that stopped where it doesn't serve: the catalogue's words, as the script said them before.
    assert_eq!(
        step_failed(StepFailed::NotServed(NotServed::DockerDistribution), false).to_string(),
        "Cyber CTF installs Docker on Debian, Ubuntu and the distributions built on them. Install Docker Engine from your distribution or https://docs.docker.com/engine/install/, then check again."
    );
    assert_eq!(
        step_failed(StepFailed::NotServed(NotServed::HypervEdition), false).to_string(),
        "This edition of Windows (Home) has no Hyper-V. Run VM labs with VirtualBox, on a server or in the cloud."
    );
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::Locked), false).to_string(), BREW_LOCKED);
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::Outdated), false).to_string(), BREW_OUTDATED);
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::XcodeTooOld), false).to_string(), BREW_XCODE_OUTDATED);
    assert_eq!(step_failed(StepFailed::Missing { program: "vagrant".into() }, false).to_string(), "vagrant is not available on this machine.");
    let exited = |known| StepFailed::Exited { program: "winget".into(), code: Some(-1_978_335_226), known };
    assert_eq!(step_failed(exited(None), false).to_string(), "`winget` exited with exit status: -1978335226");
    let cancelled = step_failed(exited(Some(Known::Cancelled)), false).to_string();
    assert!(cancelled.starts_with("The installation was cancelled") && cancelled.ends_with("(`winget` exited with exit status: -1978335226)"), "{cancelled}");
    assert!(step_failed(exited(Some(Known::Cancelled)), true).to_string().starts_with("The removal was cancelled"));
    assert!(step_failed(exited(Some(Known::NoDiskSpace)), false).to_string().starts_with("There isn't enough disk space"));
}

#[test]
fn a_step_done_although_it_exited_non_zero_reads_as_the_launcher_said_it() {
    assert_eq!(done_said(Done::AlreadyInstalled, false), "Already installed and up to date: nothing to do.");
    assert_eq!(done_said(Done::RestartToFinish, true), "Removed. Restart Windows to finish.");
    assert_eq!(done_said(Done::RestartToFinish, false), "Installed. Restart Windows to finish.");
    assert!(is_removal(&step("winget", &["uninstall", "-e"])) && !is_removal(&step("winget", &["install"])));
}

#[test]
fn isothrum_machines_notes_read_as_the_launcher_said_them() {
    use isothrum_machine::install::Dependency as Machine;
    assert_eq!(said(Note::Downloading(Machine::Docker)), "Downloading Docker Desktop…");
    assert_eq!(said(Note::Downloading(Machine::Utm)), "Downloading UTM…");
    assert_eq!(said(Note::DownloadingVmwareUtility { version: "1.0.24" }), "Downloading HashiCorp's Vagrant VMware Utility 1.0.24…");
    assert_eq!(said(Note::Removing(Machine::Vagrant)), "Removing Vagrant (macOS asks for your password)…");
    assert_eq!(said(Note::Removing(Machine::Docker)), "Running Docker Desktop's own uninstaller…");
    assert!(said(Note::QemuUpdatingHomebrewFirst).starts_with("Installing QEMU (updating Homebrew first"));
}
