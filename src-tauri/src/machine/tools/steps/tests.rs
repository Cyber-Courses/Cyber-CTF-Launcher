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
    assert!(!is_kept(&key), "the list read mid-install still stands");
}

#[cfg(unix)]
#[test]
fn each_program_is_watched_for_what_its_own_output_says() {
    // winget: the installer's code from any line of its output; other programs' lines aren't read.
    let mut w = Watch::new(&step("winget", &["install", "-e", "--id", "Docker.DockerDesktop"]));
    w.line("Installer failed with exit code: 1602");
    assert_eq!(w.installer_code, Some(1602));
    let mut w = Watch::new(&step("powershell", &["-Command", "x"]));
    w.line("Installer failed with exit code: 1602");
    assert_eq!(w.installer_code, None);
}

#[cfg(unix)]
#[tokio::test]
async fn steps_stream_both_outputs_and_report_a_failure() {
    let mut lines = Vec::new();
    let ok = Step { program: "sh".into(), args: vec!["-c".into(), "echo out; echo err >&2".into()], note: Some("Note first".into()) };
    let fail = step("sh", &["-c", "exit 3"]);
    let err = run_steps(&[ok, fail], &mut |l| lines.push(l)).await.unwrap_err();
    assert_eq!(lines[0], "Note first");
    assert!(lines.contains(&"out".to_string()) && lines.contains(&"err".to_string()));
    assert_eq!(lines.last().map(String::as_str), Some("$ sh -c exit 3"));
    assert!(err.to_string().starts_with("`sh` exited with"), "{err}");
}

#[test]
fn a_winget_step_ends_by_winget_s_codes() {
    let winget = |code: u32, installer| end_of("winget", Verb::Install, Some(code.cast_signed()), installer);
    assert_eq!(end_of("winget", Verb::Install, Some(0), None), End::Done(None));
    // Regression: nothing to do, or a restart to finish, is a step done.
    assert!(matches!(winget(0x8A15_002B, None), End::Done(Some(note)) if note.starts_with("Already installed")));
    assert!(matches!(winget(0x8A15_0109, None), End::Done(Some(note)) if note.contains("Restart Windows")));
    assert!(matches!(winget(0x8A15_0006, Some(1602)), End::Failed(Some(why)) if why.contains("cancelled")));
    assert_eq!(winget(1, None), End::Failed(None));
}

#[test]
fn another_step_ends_by_its_own_codes() {
    // Only winget's codes are winget's: another program exiting with one just failed.
    assert_eq!(end_of("sh", Verb::Install, Some(0x8A15_002B_u32.cast_signed()), None), End::Failed(None));
    assert!(matches!(end_of("powershell", Verb::Remove, Some(1223), None), End::Failed(Some(why)) if why.starts_with("The removal was cancelled")));
    assert_eq!(end_of("pkexec", Verb::Install, None, None), End::Failed(None), "killed by a signal");
}

#[cfg(unix)]
#[tokio::test]
async fn a_step_whose_output_fails_takes_what_it_started_with_it() {
    // Regression: an install step was spawned without kill_on_drop or a group, so a failed read of
    // its output left it (and its installer) running unattended.
    let file = std::env::temp_dir().join(format!("cyberctf-step-tree-{}", rand::random::<u32>()));
    let script = format!("sleep 120 & echo $! > '{}'; wait", file.display());
    let started = tokio::time::timeout(std::time::Duration::from_secs(2), run_step(&step("sh", &["-c", &script]), &mut |_| {})).await;
    assert!(started.is_err(), "cancelled while it ran");
    let pid: u32 = std::fs::read_to_string(&file).expect("the step started its child").trim().parse().unwrap();
    let _ = std::fs::remove_file(&file);
    assert!(crate::platform::exec::fake::gone(pid).await, "the step's child {pid} was left running");
}

#[tokio::test]
async fn a_missing_program_says_so() {
    let err = run_step(&step("cyberctf-no-such-tool", &[]), &mut |_| {}).await.unwrap_err();
    assert_eq!(err.to_string(), "cyberctf-no-such-tool is not available on this machine.");
}

#[test]
fn a_command_line_reads_as_typed() {
    assert_eq!(command_line(&step("brew", &["install", "qemu"])), "$ brew install qemu");
    // A proxy's password never reaches the setup log.
    let root = step("pkexec", &["env", "https_proxy=http://ada:s3cret@proxy.corp:3128", "sh", "-c", "apt-get update"]);
    assert_eq!(command_line(&root), "$ pkexec env https_proxy=http://ada:[redacted]@proxy.corp:3128 sh -c apt-get update");
}

#[test]
fn isothrum_machines_step_failures_read_as_the_launcher_said_them() {
    use isothrum_machine::install::StepFailed;
    use isothrum_machine::install::brew::Trouble;
    assert_eq!(step_failed(StepFailed::NoPkexec).to_string(), NO_PKEXEC);
    assert!(step_failed(StepFailed::PromptRefused).to_string().contains("cancelled"));
    assert!(step_failed(StepFailed::NoPolkitAgent).to_string().contains("polkit agent"));
    assert_eq!(step_failed(StepFailed::SudoRefused).to_string(), SUDO_REFUSED);
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::Locked)).to_string(), BREW_LOCKED);
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::Outdated)).to_string(), BREW_OUTDATED);
    assert_eq!(step_failed(StepFailed::Homebrew(Trouble::XcodeTooOld)).to_string(), BREW_XCODE_OUTDATED);
    assert_eq!(step_failed(StepFailed::Missing { program: "vagrant".into() }).to_string(), "vagrant is not available on this machine.");
    assert_eq!(step_failed(StepFailed::Exited { program: "pkexec".into(), code: Some(100) }).to_string(), "`pkexec` exited with exit status: 100");
}

#[test]
fn isothrum_machines_notes_read_as_the_launcher_said_them() {
    use isothrum_machine::install::{Dependency as Machine, Note};
    assert_eq!(said(Note::Downloading(Machine::Docker)), "Downloading Docker Desktop…");
    assert_eq!(said(Note::Downloading(Machine::Utm)), "Downloading UTM…");
    assert_eq!(said(Note::DownloadingVmwareUtility { version: "1.0.24" }), "Downloading HashiCorp's Vagrant VMware Utility 1.0.24…");
    assert_eq!(said(Note::Removing(Machine::Vagrant)), "Removing Vagrant (macOS asks for your password)…");
    assert_eq!(said(Note::Removing(Machine::Docker)), "Running Docker Desktop's own uninstaller…");
    assert!(said(Note::QemuUpdatingHomebrewFirst).starts_with("Installing QEMU (updating Homebrew first"));
}
