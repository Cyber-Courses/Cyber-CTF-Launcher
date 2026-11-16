use serde_json::json;

use super::*;
use isothrum_runner::Error as RunnerError;

/// The runner's report, as the service and the event stream carry it: code `code`, message `m`,
/// `step` and `detail` (its fields, version 2) when given.
fn report(code: &str, step: Option<&str>, detail: Option<serde_json::Value>) -> ErrorReport {
    let mut r = json!({ "code": code, "message": "m" });
    if let Some(step) = step {
        r["step"] = json!(step);
    }
    if let Some(mut detail) = detail {
        detail["v"] = json!(2);
        r["detail"] = detail;
    }
    serde_json::from_value(r).expect("a report the runner writes")
}

/// What the player reads for a report: the code the webview translates and the message.
fn said(r: &ErrorReport) -> (Code, String) {
    let e = coded(r);
    (e.code(), e.to_string())
}

/// Each report of `cases` reaches the webview as its code, with its message when one is given,
/// and the webview gets that same code.
fn table(cases: Vec<(ErrorReport, Code, Option<&str>)>) {
    for (r, code, message) in cases {
        let (said_code, said_message) = said(&r);
        assert_eq!(said_code, code, "{r:?}");
        if let Some(message) = message {
            assert_eq!(said_message, message, "{r:?}");
        }
        assert_eq!(coded(&r).to_wire().code, said_code, "the webview gets the same code: {r:?}");
    }
}

/// A tool's failure the runner names, with its fields, reaches the webview as the code it
/// translates, with the values its sentence needs.
#[test]
fn a_tools_failure_reaches_the_webview_as_the_code_it_translates() {
    let step = Some("up");
    table(vec![
        (
            report("port_in_use", None, Some(json!({ "kind": "port_in_use", "port": 8080, "protocol": "tcp" }))),
            Code::PortInUse { port: HostPort::new(8080), holder: None },
            None,
        ),
        (report("tool_missing", None, Some(json!({ "kind": "tool_missing", "tool": "docker" }))), Code::ToolMissing { tool: "docker".into() }, None),
        (
            report("tool_too_old", None, Some(json!({ "kind": "tool_too_old", "tool": "vagrant", "found": "2.3.8", "required": "2.4.0" }))),
            Code::ToolTooOld { tool: "Vagrant".into(), found: "2.3.8".into(), required: "2.4.0".into() },
            Some("This lab needs Vagrant 2.4.0 or later, and this computer has 2.3.8. Update it, then start the lab again."),
        ),
        (
            report("tool_failed", step, Some(json!({ "kind": "tool_failed", "reason": "exit status 1" }))),
            Code::CommandFailed { command: "lab step up".into(), stderr: "exit status 1".into() },
            Some("`lab step up` failed: exit status 1"),
        ),
        // Docker's engine stopped, or quit while a step ran (Docker Desktop quit mid-`up`).
        (report("tool_unanswered", step, Some(json!({ "kind": "tool_unanswered", "tool": "docker" }))), Code::DockerNotRunning, None),
        (report("tool_unanswered", step, Some(json!({ "kind": "tool_unanswered", "tool": "podman" }))), Code::Other, Some("m")),
        // The machine changed after its check (virtiofsd removed): the check's code, naming the
        // hypervisor as it does.
        (
            report("provider_refused", step, Some(json!({ "kind": "provider_refused", "provider": "libvirt", "need": "virtiofsd" }))),
            Code::VirtiofsdMissing { hypervisor: "libvirt".into() },
            None,
        ),
        (
            report("provider_refused", step, Some(json!({ "kind": "provider_refused", "provider": "qemu", "need": "virtiofsd" }))),
            Code::VirtiofsdMissing { hypervisor: "QEMU".into() },
            None,
        ),
        (report("provider_refused", None, Some(json!({ "kind": "provider_refused", "provider": "utm" }))), Code::Other, Some("m")),
    ]);
}

/// A step out of time, a ready condition or a server's refusal:
/// what ran out and what to do.
#[test]
fn a_steps_failure_says_what_ran_out_and_what_to_do() {
    let step = Some("up");
    table(vec![
        (
            report(
                "timeout",
                step,
                Some(json!({ "kind": "timeout", "timed_out": { "kind": "no_progress", "after_s": 1800, "last_line": "web Downloading 1.121GB" } })),
            ),
            Code::StepStalled { step: "up".into(), minutes: 30, last_line: "web Downloading 1.121GB".into() },
            Some(
                "The lab's `up` step showed no progress for 30 minutes, so it was stopped. Its last output was: \u{201c}web Downloading 1.121GB\u{201d}. Start the lab again; if it keeps happening, check the network and the resources of Docker, the hypervisor or the server.",
            ),
        ),
        (
            report("timeout", step, Some(json!({ "kind": "timeout", "timed_out": { "kind": "no_progress", "after_s": 1800 } }))),
            Code::StepSilent { step: "up".into(), minutes: 30 },
            None,
        ),
        (
            report("timeout", step, Some(json!({ "kind": "timeout", "timed_out": { "kind": "ceiling", "after_s": 21600 } }))),
            Code::StepCeiling { step: "up".into(), hours: 6 },
            None,
        ),
        (
            report("server_auth_failed", step, Some(json!({ "kind": "server_auth_failed", "server": "10.0.0.5" }))),
            Code::ServerAuthFailed { server: "10.0.0.5".into() },
            None,
        ),
        (
            report("elevation_failed", None, Some(json!({ "kind": "elevation", "reason": "the prompt was declined" }))),
            Code::ElevationFailed { reason: "the prompt was declined".into() },
            Some("Hyper-V needs administrator rights, and they couldn't be had (the prompt was declined). Start the lab again and accept the prompt."),
        ),
        (
            report("not_ready", None, Some(json!({ "kind": "not_ready", "condition": "tcp 127.0.0.1:8080", "detail": "connection refused" }))),
            Code::LabNotReady { detail: "connection refused".into() },
            None,
        ),
    ]);
}

/// A download that stalled or failed says so, never the resources (an 11 GB attack box through a
/// slow connection was once told to check Docker's), and a Vagrant box is never "the lab's
/// images".
#[test]
fn a_download_that_stalled_or_failed_says_what_it_fetched() {
    let step = Some("up");
    table(vec![
        (
            report("download_stalled", Some("pull-up"), Some(json!({ "kind": "download_stalled", "what": "images", "after_s": 900 }))),
            Code::DownloadStalled { minutes: 15 },
            Some(
                "Downloading the lab's images stopped making progress (nothing new for 15 minutes), so it was stopped. Check your internet connection, then try again: what was already downloaded is kept.",
            ),
        ),
        (
            report(
                "download_failed",
                Some("pull-up"),
                Some(json!({ "kind": "download_failed", "what": "images", "reason": "toomanyrequests: You have reached your pull rate limit\n" })),
            ),
            Code::DownloadFailed { reason: "toomanyrequests: You have reached your pull rate limit".into() },
            Some("Couldn't download the lab's images: toomanyrequests: You have reached your pull rate limit. Check your internet connection, then try again."),
        ),
        // A Vagrant box: never said as "the lab's images".
        (
            report("download_stalled", step, Some(json!({ "kind": "download_stalled", "what": "boxes", "after_s": 900 }))),
            Code::BoxDownloadStalled { minutes: 15 },
            Some(
                "Downloading the lab's VM box stopped making progress (nothing new for 15 minutes), so it was stopped. Check your internet connection, then try again.",
            ),
        ),
        (
            report(
                "download_failed",
                step,
                Some(json!({ "kind": "download_failed", "what": "boxes", "reason": "The box 'bento/debian-12' could not be found.\n" })),
            ),
            Code::BoxDownloadFailed { reason: "The box 'bento/debian-12' could not be found".into() },
            Some("Couldn't download the lab's VM box: The box 'bento/debian-12' could not be found. Check your internet connection, then try again."),
        ),
    ]);
}

/// The refusals say what to do in their own words, and anything else keeps the runner's.
#[test]
fn a_refusal_or_the_runners_own_words_keep_the_sentence_the_player_reads() {
    let step = Some("up");
    table(vec![
        (
            report("nothing_to_resume", None, Some(json!({ "kind": "partly_gone", "machines": ["web", "db"] }))),
            Code::MachinesGone { machines: "web, db".into() },
            Some(
                "Some of this lab's machines are gone (web, db): they were removed outside Cyber CTF, so it can't resume as it was. Start it again for a fresh copy.",
            ),
        ),
        (report("nothing_to_resume", None, None), Code::NothingToResume, None),
        (report("locked", None, None), Code::AlreadyInProgress, None),
        (report("not_parkable", None, Some(json!({ "kind": "not_parkable", "mode": "pause" }))), Code::NotParkable, None),
        // The refusals say what to do, in their own words.
        (report("already_running", None, None), Code::Refused, Some("This lab is already running on this machine. Stop it before starting it again.")),
        (report("running_elsewhere", None, None), Code::Refused, Some("This lab is still running somewhere else. Stop it there before starting it here.")),
        (report("not_running", None, None), Code::RefusedBeforeActing, Some("This lab isn't running. Start it first.")),
        (
            report("secret_missing", None, Some(json!({ "kind": "secret_missing", "name": "CTF_FLAG_MAIN" }))),
            Code::RefusedBeforeActing,
            Some("This lab needs a value for `CTF_FLAG_MAIN` that the launch didn't give. Start it from the catalogue again."),
        ),
        (
            report("input_invalid", None, Some(json!({ "kind": "input_invalid", "name": "ISOLOOM_DERIVED", "reason": "`2` isn't one of 0, 1" }))),
            Code::RefusedBeforeActing,
            Some("This lab needs a value for `ISOLOOM_DERIVED` it can use: the one given `2` isn't one of 0, 1. Start it from the catalogue again."),
        ),
        (
            report(
                "credentials_expired",
                Some("destroy"),
                Some(json!({ "kind": "credentials_expired", "cloud": "aws", "reason": "login session has expired, please reauthenticate" })),
            ),
            Code::RefusedBeforeActing,
            Some(
                "Your sign-in to AWS has expired or is missing, so the lab wasn't changed. Sign in again (`aws login`, or `aws sso login`), then try again. (login session has expired, please reauthenticate)",
            ),
        ),
        (
            report(
                "address_pool_overlap",
                step,
                Some(json!({ "kind": "address_pool_overlap", "reason": "invalid pool request: Pool overlaps with other one on this address space" })),
            ),
            Code::Other,
            Some(
                "Another Docker network already uses the addresses this lab needs. Stop the other lab, or remove the network left behind (`docker network prune` removes the unused ones), then start this lab again. (`lab step up` failed: invalid pool request: Pool overlaps with other one on this address space)",
            ),
        ),
        (report("unsupported", None, None), Code::Other, Some("Cyber CTF can't run this lab's generated files: m")),
        (report("project_invalid", None, None), Code::Other, Some("Cyber CTF can't run this lab's generated files: m")),
        // Anything else keeps the runner's words.
        (report("machine_unknown", None, None), Code::Other, Some("m")),
    ]);
}

/// Every code the runner has today is mapped by name; only a code a later runner adds falls back
/// to its own message.
#[test]
fn every_code_the_runner_has_is_mapped_by_name() {
    let codes = isothrum_runner::every_name::<ErrorCode>();
    assert!(codes.len() > 30, "{codes:?}");
    for code in codes {
        assert!(known(&report(&code, None, None)).is_some(), "`{code}` has no arm of its own in `known`");
    }
}

#[test]
fn docker_refusing_this_account_says_how_to_get_access() {
    let denied = coded(&report("tool_denied", Some("up"), Some(json!({ "kind": "tool_denied", "tool": "docker" })))).code();
    assert!(matches!(denied, Code::DockerDenied { .. } | Code::DockerDeniedWindows { .. }), "{denied:?}");
}

#[test]
fn a_cloud_signed_in_with_a_stored_token_or_none_named_has_no_cli_sign_in_to_name() {
    for cloud in [Some("linode"), None] {
        let detail = cloud
            .map_or_else(|| json!({ "kind": "credentials_expired", "reason": "r" }), |c| json!({ "kind": "credentials_expired", "cloud": c, "reason": "r" }));
        let (_, message) = said(&report("credentials_expired", None, Some(detail)));
        assert!(message.starts_with("Your sign-in to the cloud account has expired or is missing") && message.contains("Sign in again, then"), "{message}");
    }
    let (_, gcp) = said(&report("credentials_expired", None, Some(json!({ "kind": "credentials_expired", "cloud": "gcp", "reason": "r" }))));
    assert!(gcp.starts_with("Your sign-in to Google Cloud"), "{gcp}");
}

#[test]
fn a_taken_port_names_its_holder_as_the_player_knows_it() {
    for (program, holder) in [("nginx", "nginx (pid 700)"), ("com.docke", "Docker (pid 700)"), ("docker-pr", "Docker (pid 700)")] {
        let taken =
            report("port_in_use", None, Some(json!({ "kind": "port_in_use", "port": 8080, "protocol": "tcp", "holder": { "pid": 700, "program": program } })));
        assert_eq!(coded(&taken).code(), Code::PortInUse { port: HostPort::new(8080), holder: Some(holder.into()) }, "{program}");
    }
}

/// Regression (docs/contributing/testing/resilience.md N4): a revoked Proxmox token or a changed
/// ESXi password showed only as the step's raw failure.
#[test]
fn a_server_refusing_the_credentials_is_named_as_servers_names_it() {
    let refused = coded(&report("server_auth_failed", Some("status"), Some(json!({ "kind": "server_auth_failed" }))));
    assert_eq!(refused.code(), Code::ServerAuthFailed { server: "the server".into() });
    let named = on_server(refused, "Home lab");
    assert_eq!(named.code(), Code::ServerAuthFailed { server: "Home lab".into() });
    assert_eq!(named.to_string(), "Cyber CTF can't sign in to Home lab: its token was revoked or its password changed. Update it in Servers.");
    // Any other failure keeps its own words.
    let other = on_server(coded(&report("not_running", None, None)), "Home lab");
    assert!(!other.to_string().contains("sign in"), "{other}");
}

#[test]
fn a_helper_vm_the_runner_no_longer_opens_says_what_is_left_and_how_to_remove_it() {
    let gone = RunnerError::Unsupported { what: "containers run inside the run's VM".into() };
    let said = of_helper_vm(&gone.report(), "cyberctf-web").to_string();
    assert!(said.starts_with(HELPER_VM_LEFT) && said.contains("cyberctf-web"), "{said}");
    let locked = RunnerError::Locked { run: "r".into() };
    assert_eq!(of_helper_vm(&locked.report(), "r").code(), coded(&locked.report()).code());
}
