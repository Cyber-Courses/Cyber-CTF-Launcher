use super::*;

fn said(e: RunnerError) -> (Code, String) {
    let e = from_runner(e);
    (e.code(), e.to_string())
}

#[test]
fn a_taken_port_and_a_missing_tool_keep_the_codes_the_ui_knows() {
    assert_eq!(
        said(RunnerError::PortInUse { port: isothrum_runner::HostPort::new(8080), protocol: "tcp".into() }).0,
        Code::PortInUse { port: HostPort::new(8080), holder: None }
    );
    assert_eq!(said(RunnerError::ToolMissing { step: "tools".into(), tool: "docker".into() }).0, Code::ToolMissing { tool: "docker".into() });
    assert_eq!(said(RunnerError::Locked { run: "cyberctf-web".into() }).0, Code::AlreadyInProgress);
}

#[test]
fn docker_not_answering_its_probe_reads_as_a_stopped_engine() {
    let unanswered = |reason: &str| RunnerError::ToolUnanswered { step: "tools".into(), tool: "docker".into(), reason: reason.into() };
    assert_eq!(said(unanswered("Cannot connect to the Docker daemon")).0, Code::DockerNotRunning);
    assert!(matches!(said(unanswered("permission denied while trying to connect")).0, Code::DockerDenied { .. } | Code::DockerDeniedWindows { .. }));
    // The service's answer: the code and its tool, the runner's whole message.
    let served = |tool: &str| {
        let e = RunnerError::ToolUnanswered { step: "tools".into(), tool: tool.into(), reason: "Cannot connect to the Docker daemon".into() };
        from_service(e.code(), &e.to_string(), Some("tools"), e.detail().as_ref()).code()
    };
    assert_eq!(served("docker"), Code::DockerNotRunning);
    // Another tool's silence isn't Docker's.
    assert_ne!(served("podman"), Code::DockerNotRunning);
}

#[test]
fn docker_stopped_while_a_step_ran_reads_as_a_stopped_engine_on_this_machine_only() {
    // Regression: Docker Desktop quit while the lab's `compose up` ran, and the lab said "`lab
    // step up` failed: Cannot connect to the Docker daemon…" rather than to start Docker.
    for stderr in [
        "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
        "error during connect: Get \"http://%2F%2F.%2Fpipe%2FdockerDesktopLinuxEngine/v1.47/containers/json\": open //./pipe/dockerDesktopLinuxEngine: The system cannot find the file specified.",
    ] {
        let failed = from_runner(RunnerError::ToolFailed { step: "up".into(), reason: stderr.into() });
        assert_eq!(engine_stopped(failed).code(), Code::DockerNotRunning, "{stderr}");
    }
    // A daemon that answers but refuses this account, or any other failure, keeps its words.
    let denied =
        from_runner(RunnerError::ToolFailed { step: "up".into(), reason: "error during connect: open //./pipe/docker_engine: Access is denied.".into() });
    assert!(matches!(engine_stopped(denied).code(), Code::CommandFailed { .. }));
    let other = from_runner(RunnerError::ToolFailed { step: "up".into(), reason: "exit status 1".into() });
    assert!(matches!(engine_stopped(other).code(), Code::CommandFailed { .. }));
}

#[test]
fn a_resume_with_machines_removed_outside_names_them_and_says_to_start_again() {
    let e = from_runner(RunnerError::PartlyGone { run: "cyberctf-web".into(), machines: vec!["web".into(), "db".into()] });
    assert_eq!(
        e.to_string(),
        "Some of this lab's machines are gone (web, db): they were removed outside Cyber CTF, so it can't resume as it was. Start it again for a fresh copy."
    );
    assert!(!refused_before_acting(&e), "the run was recorded failed: the next start clears it");
}

#[test]
fn a_failed_step_says_which_and_why_and_a_pool_overlap_what_to_do() {
    let (code, text) = said(RunnerError::ToolFailed { step: "up".into(), reason: "exit status 1".into() });
    assert_eq!(code, Code::CommandFailed { command: "lab step up".into(), stderr: "exit status 1".into() });
    assert_eq!(text, "`lab step up` failed: exit status 1");
    let overlap = RunnerError::ToolFailed { step: "up".into(), reason: "invalid pool request: Pool overlaps with other one on this address space".into() };
    assert!(said(overlap).1.starts_with("Another Docker network already uses the addresses this lab needs."));
}

#[test]
fn a_cloud_is_known_by_its_terraform_provider_and_named_by_its_label() {
    assert_eq!(cloud_in("provider[\"registry.terraform.io/hashicorp/azurerm\"]"), Some(Provider::Azure));
    assert_eq!(cloud_in("provider[\"registry.terraform.io/oracle/oci\"]"), Some(Provider::Oci));
    assert_eq!(cloud_in("nothing of a cloud"), None);
    assert_eq!(sign_in_of(Provider::Gcp).map(|(name, _)| name), Some("Google Cloud"));
    // A cloud signed in with a stored API token has no CLI sign-in to make again.
    assert_eq!(sign_in_of(Provider::DigitalOcean), None);
    assert_eq!(sign_in_of(Provider::Linode), None);
}

#[test]
fn the_lifecycle_refusals_say_what_to_do() {
    let (code, text) = said(RunnerError::AlreadyRunning { run: "cyberctf-web".into() });
    assert_eq!((code, text.as_str()), (Code::Refused, "This lab is already running on this machine. Stop it before starting it again."));
    assert!(matches!(from_runner(RunnerError::AlreadyRunning { run: "r".into() }), Error::Refused(_)), "a refusal leaves the last verdict as it was");
    assert!(said(RunnerError::NothingToResume { run: "r".into() }).1.starts_with("Nothing is left of this lab to resume"));
    assert!(said(RunnerError::NotRunning { run: "r".into() }).1.contains("Start it first"));
    let expired = "with provider[\"registry.terraform.io/hashicorp/aws\"] Error: failed to refresh cached credentials, create oauth2 token: login session has expired, please reauthenticate (exited with code 1)";
    let said_expired = said(RunnerError::ToolFailed { step: "destroy".into(), reason: expired.into() }).1;
    assert!(said_expired.starts_with("Your sign-in to AWS has expired") && said_expired.contains("`aws login`"), "{said_expired}");
    for (reason, cloud, again) in [
        ("ERROR: AADSTS700082: The refresh token has expired due to inactivity. Please run 'az login'", Provider::Azure, "`az login`"),
        ("Reauthentication failed. cannot prompt during non-interactive execution.", Provider::Gcp, "`gcloud auth application-default login`"),
        ("ServiceError: NotAuthenticated: The required information to complete authentication was not provided", Provider::Oci, "`oci session authenticate`"),
    ] {
        let said = expired_sign_in(reason, Some(cloud)).expect("an expired sign-in").to_string();
        assert!(said.contains(again) && said.contains(cloud.label()), "{said}");
    }
    assert!(expired_sign_in("Error: creating EC2 Instance: InsufficientInstanceCapacity", Some(Provider::Aws)).is_none());
    let stalled = TimedOut::NoProgress { after_s: 1800, last_line: Some("web Downloading 1.121GB".into()) };
    assert_eq!(
        said(RunnerError::Timeout { step: "up".into(), timed_out: stalled }).1,
        "The lab's `up` step showed no progress for 30 minutes, so it was stopped. Its last output was: \u{201c}web Downloading 1.121GB\u{201d}. Start the lab again; if it keeps happening, check the network and the resources of Docker, the hypervisor or the server."
    );
    let silent = TimedOut::NoProgress { after_s: 1800, last_line: None };
    assert!(said(RunnerError::Timeout { step: "up".into(), timed_out: silent }).1.starts_with("The lab's `up` step printed nothing for 30 minutes"));
    assert!(said(RunnerError::SecretMissing { name: "CTF_FLAG_MAIN".into() }).1.contains("`CTF_FLAG_MAIN`"));
}

#[test]
fn the_runners_failures_the_launcher_words_reach_the_webview_as_codes_it_translates() {
    let stalled = TimedOut::NoProgress { after_s: 1800, last_line: Some("pulling".into()) };
    assert_eq!(
        said(RunnerError::Timeout { step: "up".into(), timed_out: stalled }).0,
        Code::StepStalled { step: "up".into(), minutes: 30, last_line: "pulling".into() }
    );
    let ceiling = TimedOut::Ceiling { after_s: 21600, last_line: None };
    assert_eq!(said(RunnerError::Timeout { step: "up".into(), timed_out: ceiling }).0, Code::StepCeiling { step: "up".into(), hours: 6 });
    let old = said(RunnerError::ToolTooOld { tool: "vagrant".into(), found: "2.3".into(), required: "2.4".into() }).0;
    assert_eq!(old, Code::ToolTooOld { tool: "Vagrant".into(), found: "2.3".into(), required: "2.4".into() });
    assert_eq!(said(RunnerError::Elevation { reason: "declined".into() }).0, Code::ElevationFailed { reason: "declined".into() });
    let gone = RunnerError::PartlyGone { run: "r".into(), machines: vec!["web".into(), "db".into()] };
    assert_eq!(said(gone).0, Code::MachinesGone { machines: "web, db".into() });
    assert_eq!(said(RunnerError::NothingToResume { run: "r".into() }).0, Code::NothingToResume);
}

#[test]
fn a_download_that_stalled_or_failed_says_so_not_the_resources() {
    // Regression: an 11 GB attack box still downloading through a slow connection was
    // stopped with the start's `up`, and the player was told to check Docker's resources.
    let stalled = RunnerError::DownloadStalled { step: "pull-up".into(), after_s: 900, last_line: Some("36b0bafd7f97 Downloading 1.283GB".into()) };
    let (code, text) = said(stalled);
    assert_eq!(code, Code::DownloadStalled { minutes: 15 });
    assert_eq!(
        text,
        "Downloading the lab's images stopped making progress (nothing new for 15 minutes), so it was stopped. Check your internet connection, then try again: what was already downloaded is kept."
    );
    let failed = RunnerError::DownloadFailed {
        step: "pull-up".into(),
        reason: "Error response from daemon: toomanyrequests: You have reached your pull rate limit\n".into(),
    };
    let (code, text) = said(failed);
    assert_eq!(code, Code::DownloadFailed { reason: "Error response from daemon: toomanyrequests: You have reached your pull rate limit".into() });
    assert_eq!(
        text,
        "Couldn't download the lab's images: Error response from daemon: toomanyrequests: You have reached your pull rate limit. Check your internet connection, then try again."
    );
    for text in [said(RunnerError::DownloadStalled { step: "pull-up".into(), after_s: 900, last_line: None }).1, text] {
        assert!(!text.contains("resources"), "{text}");
    }
    // A step that ran out of time still says to check the resources.
    let up = TimedOut::NoProgress { after_s: 1800, last_line: None };
    assert!(said(RunnerError::Timeout { step: "up".into(), timed_out: up }).1.contains("the resources of Docker"));
}

/// Regression: a Vagrant box download that stalled or failed (the runner's `boxes-<step>`)
/// was told as "the lab's images", a container lab's words.
#[test]
fn a_vm_box_download_that_stalled_or_failed_is_said_as_the_box() {
    let stalled = RunnerError::DownloadStalled { step: "boxes-up".into(), after_s: 900, last_line: Some("box: Progress: 41% (Rate: 0/s)".into()) };
    let (code, text) = said(stalled);
    assert_eq!(code, Code::BoxDownloadStalled { minutes: 15 });
    assert_eq!(
        text,
        "Downloading the lab's VM box stopped making progress (nothing new for 15 minutes), so it was stopped. Check your internet connection, then try again."
    );
    let failed = RunnerError::DownloadFailed { step: "boxes-up".into(), reason: "The box 'bento/debian-12' could not be found.\n".into() };
    let (code, text) = said(failed);
    assert_eq!(code, Code::BoxDownloadFailed { reason: "The box 'bento/debian-12' could not be found".into() });
    assert_eq!(text, "Couldn't download the lab's VM box: The box 'bento/debian-12' could not be found. Check your internet connection, then try again.");
    // A lab's own download step, or the images' pull, is still the lab's images.
    for step in ["pull-up", "pull-start", "boxes"] {
        let (code, _) = said(RunnerError::DownloadStalled { step: step.into(), after_s: 60, last_line: None });
        assert_eq!(code, Code::DownloadStalled { minutes: 1 }, "{step}");
    }
}

/// Regression (tests/resilience.md N4): a revoked Proxmox token or a changed ESXi password
/// showed only as the step's raw failure.
#[test]
fn a_server_refusing_the_credentials_says_to_update_them_in_servers() {
    let refused = RunnerError::ServerAuthFailed {
        step: "up".into(),
        server: Some("10.0.0.5".into()),
        reason: "received an HTTP 401 response - Reason: authentication failure".into(),
    };
    let (code, _) = said(refused);
    assert_eq!(code, Code::ServerAuthFailed { server: "10.0.0.5".into() });
    let named = on_server(from_runner(RunnerError::ServerAuthFailed { step: "status".into(), server: None, reason: "401".into() }), "Home lab");
    assert_eq!(named.code(), Code::ServerAuthFailed { server: "Home lab".into() });
    assert_eq!(named.to_string(), "Cyber CTF can't sign in to Home lab: its token was revoked or its password changed. Update it in Servers.");
    // Any other failure keeps its own words.
    let other = on_server(from_runner(RunnerError::NotRunning { run: "r".into() }), "Home lab");
    assert!(!other.to_string().contains("sign in"), "{other}");
}

#[test]
fn a_vm_labs_own_failures_say_what_to_do() {
    let old = RunnerError::ToolTooOld { tool: "vagrant".into(), found: "2.3.8".into(), required: "2.4.0".into() };
    assert_eq!(said(old).1, "This lab needs Vagrant 2.4.0 or later, and this computer has 2.3.8. Update it, then start the lab again.");
    let declined = RunnerError::Elevation { reason: "the prompt was declined".into() };
    assert_eq!(
        said(declined).1,
        "Hyper-V needs administrator rights, and they couldn't be had (the prompt was declined). Start the lab again and accept the prompt."
    );
    let ceiling = TimedOut::Ceiling { after_s: 21600, last_line: None };
    assert!(
        said(RunnerError::Timeout { step: "up".into(), timed_out: ceiling })
            .1
            .starts_with("The lab's `up` step was still working after 6 hours, the longest any step may run")
    );
}

#[test]
fn the_remaining_runner_failures_each_get_their_wording() {
    // A run recorded elsewhere is a refusal (the last verdict stays), worded as what to do.
    let elsewhere = from_runner(RunnerError::RunningElsewhere { run: "r".into(), recorded: "A".into(), asked: "B".into() });
    assert!(matches!(elsewhere, Error::Refused(_)));
    assert!(elsewhere.to_string().contains("still running somewhere else"), "{elsewhere}");
    // A lab with no park steps, and files the runner can't run, are plain messages.
    assert!(said(RunnerError::NotParkable { run: "r".into(), mode: "pause" }).1.contains("can't be paused or shut down"));
    assert!(said(RunnerError::Unsupported { what: "driver vagrant".into() }).1.contains("can't run this lab's generated files"));
    // A ready condition that never held keeps its detail.
    assert!(
        said(RunnerError::NotReady { condition: "tcp 127.0.0.1:8080".into(), detail: "connection refused".into() })
            .1
            .contains("didn't become ready: connection refused")
    );
    // `tool_name` turns the runner's short id into the player's name (Docker, Docker Compose),
    // and quotes an unknown one verbatim.
    let docker = said(RunnerError::ToolTooOld { tool: "docker".into(), found: "20".into(), required: "24".into() }).1;
    assert!(docker.starts_with("This lab needs Docker 24 or later"), "{docker}");
    let compose = said(RunnerError::ToolTooOld { tool: "docker-compose".into(), found: "1".into(), required: "2".into() }).1;
    assert!(compose.contains("Docker Compose 2 or later"), "{compose}");
    let other = said(RunnerError::ToolTooOld { tool: "qemu".into(), found: "6".into(), required: "8".into() }).1;
    assert!(other.contains("`qemu` 8 or later"), "{other}");
}

#[test]
fn anything_else_keeps_the_runners_words() {
    let e = RunnerError::MachineUnknown { run: "r".into(), machine: "web".into() };
    let text = e.to_string();
    assert_eq!(said(e).1, text);
    // A folder that isn't a project the runner runs: its generated files are wrong.
    let invalid = RunnerError::Project { dir: "lab".into(), reason: "no compose.yml".into() };
    let text = invalid.to_string();
    assert_eq!(said(invalid).1, format!("Cyber CTF can't run this lab's generated files: {text}"));
}

#[test]
fn a_missing_or_refused_value_changed_nothing() {
    // The runner checks the launch's values before it runs anything: a change refused for one
    // is rolled back like one refused for a run that isn't up.
    for e in [
        RunnerError::SecretMissing { name: "CTF_FLAG_MAIN".into() },
        RunnerError::Input { name: "ISOLOOM_DERIVED".into(), reason: "`2` isn't one of 0, 1".into() },
    ] {
        let launcher = from_runner(e);
        assert!(refused_before_acting(&launcher), "{launcher}");
    }
    let param = from_runner(RunnerError::Input { name: "ISOLOOM_DERIVED".into(), reason: "`2` isn't one of 0, 1".into() });
    assert!(param.to_string().contains("`ISOLOOM_DERIVED`") && param.to_string().contains("`2` isn't one of 0, 1"), "{param}");
    assert!(!refused_before_acting(&Error::Invalid("a step failed".into())));
}

#[test]
fn a_refusal_is_known_by_its_kind_not_its_words() {
    // The run not up, a cloud sign-in that ran out, a busy run, a missing tool, a run up
    // already: refused before acting, whatever they say.
    for refused in [
        from_runner(RunnerError::NotRunning { run: "r".into() }),
        expired_sign_in("Error: login session has expired", Some(Provider::Aws)).expect("an expired sign-in"),
        from_runner(RunnerError::Locked { run: "r".into() }),
        from_runner(RunnerError::ToolMissing { step: "tools".into(), tool: "vagrant".into() }),
        from_runner(RunnerError::AlreadyRunning { run: "r".into() }),
    ] {
        assert!(refused_before_acting(&refused), "{refused}");
        assert_eq!(refused.code(), refused.to_wire().code, "the UI gets the same code: {refused}");
    }
    // The same words from anything else are no refusal.
    assert!(!refused_before_acting(&Error::Invalid("This lab isn't running. Start it first.".into())));
    let failed_step = from_runner(RunnerError::ToolFailed { step: "up".into(), reason: "exit status 1".into() });
    assert!(!refused_before_acting(&failed_step), "{failed_step}");
}

#[test]
fn a_helper_vm_the_runner_no_longer_opens_says_what_is_left_and_how_to_remove_it() {
    let gone = RunnerError::Unsupported { what: "containers run inside the run's VM".into() };
    let said = of_helper_vm(gone, "cyberctf-web").to_string();
    assert!(said.starts_with(HELPER_VM_LEFT) && said.contains("cyberctf-web"), "{said}");
    assert_eq!(of_helper_vm(RunnerError::Locked { run: "r".into() }, "r").code(), from_runner(RunnerError::Locked { run: "r".into() }).code());
}

#[test]
fn utms_passing_scripting_errors_are_tried_again_and_said_and_nothing_else_is() {
    let mut said = Vec::new();
    let mut log = |l: String| said.push(l);
    let refused = Error::Invalid(
            "`vagrant reload default --no-provision` failed: read_forwarded_ports.applescript: execution error: UTM got an error: Application isn\u{2019}t running. (-600)".into(),
        );
    assert!(try_again_after_utm(&refused, &mut log));
    // Busy, UTM lets the provider's AppleEvent time out: as passing.
    let timed_out = Error::Invalid(
        "`vagrant up` failed: osascript read_forwarded_ports.applescript: execution error: UTM got an error: AppleEvent timed out. (-1712)".into(),
    );
    assert!(try_again_after_utm(&timed_out, &mut log));
    assert!(!try_again_after_utm(&Error::Invalid("a step failed".into()), &mut log));
    assert_eq!(said, [utm_retry_line("\"Application isn't running\" while it runs"), utm_retry_line("\"AppleEvent timed out\" while busy")]);
}

#[test]
fn libvirt_finding_no_virtiofsd_reads_as_its_code_not_vagrant_libvirts_stack_trace() {
    // Regression: a lab with the attack box's shared folder on libvirt ended on the raw Ruby
    // stack trace ("Call to virDomainCreate failed: operation failed: Unable to find a satisfying
    // virtiofsd (Libvirt::Error)"). The check refuses it up front; a machine changed since gets
    // the same code from the runner's failure, here and through the service.
    let trace = include_str!("../../../platform/tool_output/fixtures/vagrant/libvirt-no-virtiofsd.txt");
    let (code, text) = said(RunnerError::ToolFailed { step: "up".into(), reason: trace.into() });
    assert_eq!(code, Code::VirtiofsdMissing);
    assert!(text.contains("virtiofsd") && !text.contains("Libvirt::Error"), "{text}");
    let e = RunnerError::ToolFailed { step: "up".into(), reason: trace.into() };
    assert_eq!(from_service(e.code(), &e.to_string(), Some("up"), e.detail().as_ref()).code(), Code::VirtiofsdMissing);
}
