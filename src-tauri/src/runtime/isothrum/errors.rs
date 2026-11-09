//! The runner's failures as the player reads them. The runner's messages are plain and technical,
//! with a stable code; the launcher's wording (and the codes its UI knows) is decided here, from
//! that code, the same as the launcher's own Docker path says it.

use isothrum_runner::{Error as RunnerError, ErrorCode, TimedOut};

use crate::error::{Code, Error};

/// Whether `e` is a change of a run refused before acting on it: refused as such (the run isn't
/// up, a value it needs is missing or unusable — the runner checks the launch's values before it
/// runs anything —, the cloud account's sign-in ran out, asked before the change:
/// [`Error::RefusedBeforeActing`]; the run already up or recorded elsewhere: [`Error::Refused`]),
/// another operation holds it, or a tool it needs is missing. Nothing of the run changed then.
pub(crate) fn refused_before_acting(e: &Error) -> bool {
    matches!(e, Error::RefusedBeforeActing(_) | Error::Refused(_)) || matches!(e.code(), Code::AlreadyInProgress | Code::ToolMissing { .. })
}

/// What a step stopped for taking too long says: how long it went without progress (and what it
/// last printed), or that it hit the longest any step may run.
fn timed_out_step(step: &str, timed_out: &TimedOut) -> Error {
    const AGAIN: &str = "Start the lab again; if it keeps happening, check the network and the resources of Docker, the hypervisor or the server.";
    let minutes = timed_out.after_s().div_ceil(60);
    Error::Invalid(match timed_out {
        TimedOut::NoProgress { last_line: Some(line), .. } => {
            format!("The lab's `{step}` step showed no progress for {minutes} minutes, so it was stopped. Its last output was: \u{201c}{line}\u{201d}. {AGAIN}")
        }
        TimedOut::NoProgress { last_line: None, .. } => {
            format!("The lab's `{step}` step printed nothing for {minutes} minutes, so it was stopped. {AGAIN}")
        }
        TimedOut::Ceiling { after_s } => format!(
            "The lab's `{step}` step was still working after {} hours, the longest any step may run, so it was stopped. {AGAIN}",
            after_s.div_ceil(3600)
        ),
        // A reason added later reads as the plain limit.
        _ => format!("The lab's `{step}` step ran into its {minutes}-minute limit, so it was stopped. {AGAIN}"),
    })
}

/// A lab whose files another version of the launcher made, and this one couldn't make again.
pub(super) const EARLIER_FILES: &str = "This lab's files were made by another version of Cyber CTF, and they couldn't be made again for this one";

/// What to do about a lab whose files can't be made again.
pub(super) const ADD_AGAIN: &str = "Remove the lab and add it again from the catalogue; anything it still runs is on the Machine page.";

/// The launcher's error for a runner's failure.
pub(super) fn from_runner(e: RunnerError) -> Error {
    match e {
        // The holder is filled in later, on the launch path, where an async lookup can run.
        RunnerError::PortInUse { port, .. } => Error::coded(Code::PortInUse { port, holder: None }),
        RunnerError::ToolMissing { tool, .. } => Error::coded(Code::ToolMissing { tool }),
        RunnerError::ToolFailed { step, reason } => tool_failed(&step, &reason),
        RunnerError::Timeout { step, timed_out } => timed_out_step(&step, &timed_out),
        RunnerError::ToolTooOld { tool, found, required } => Error::Invalid(format!(
            "This lab needs {} {required} or later, and this computer has {found}. Update it, then start the lab again.",
            tool_name(&tool)
        )),
        RunnerError::Elevation { .. } => {
            Error::Invalid("Hyper-V needs administrator rights, and the permission prompt was declined. Start the lab again and accept the prompt.".into())
        }
        RunnerError::NotReady { detail, .. } => Error::Invalid(format!("The lab started, but didn't become ready: {detail}")),
        RunnerError::SecretMissing { name } => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` that the launch didn't give. Start it from the catalogue again."))
        }
        RunnerError::Param { name, reason } => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` it can use: the one given {reason}. Start it from the catalogue again."))
        }
        other => by_code(&other),
    }
}

/// The failures whose wording needs nothing but their code (their message is kept otherwise).
fn by_code(e: &RunnerError) -> Error {
    match e.code() {
        ErrorCode::AlreadyRunning => Error::Refused("This lab is already running on this machine. Stop it before starting it again.".into()),
        ErrorCode::RunningElsewhere => Error::Refused("This lab is still running somewhere else. Stop it there before starting it here.".into()),
        ErrorCode::Locked => Error::coded(Code::AlreadyInProgress),
        ErrorCode::NothingToResume => {
            Error::Invalid("Nothing is left of this lab to resume (its stopped containers or VMs are gone). Start it again for a fresh copy.".into())
        }
        ErrorCode::NotRunning => Error::RefusedBeforeActing("This lab isn't running. Start it first.".into()),
        ErrorCode::NotParkable => Error::Invalid("This lab can't be paused or shut down. Stop it instead.".into()),
        // A lab's files of another version are generated again before the runner reads them
        // (`regenerate`): one still refused is a lab that can't be generated any more.
        ErrorCode::ManifestVersion => Error::Invalid(format!("{EARLIER_FILES}. {ADD_AGAIN} ({e})")),
        ErrorCode::ManifestInvalid | ErrorCode::Unsupported => Error::Invalid(format!("Cyber CTF can't run this lab's generated files: {e}")),
        _ => Error::Invalid(e.to_string()),
    }
}

/// A tool as the player knows it (`vagrant` is Vagrant).
fn tool_name(tool: &str) -> String {
    match tool {
        "vagrant" => "Vagrant".into(),
        "docker" => "Docker".into(),
        "docker-compose" => "Docker Compose".into(),
        other => format!("`{other}`"),
    }
}

/// What a cloud's tools say when the sign-in they use has run out or is missing: Terraform's
/// providers and the clouds' CLIs, from a CLI login or SSO session, an expired session token,
/// application-default credentials gone stale, or a profile that was never signed in.
const EXPIRED_SIGN_IN: [&str; 21] = [
    // AWS
    "login session has expired",
    "please reauthenticate",
    "expiredtoken",
    "security token included in the request is expired",
    "token has expired",
    "error loading sso token",
    "unable to locate credentials",
    "aws login",
    // Azure
    "az login",
    "aadsts700082",
    "aadsts50173",
    "refresh token has expired",
    "interactive authentication is needed",
    // Google Cloud
    "reauthentication failed",
    "problem refreshing your current auth tokens",
    "gcloud auth application-default login",
    "could not find default credentials",
    "invalid_grant",
    // Oracle Cloud
    "session is no longer valid",
    "oci session authenticate",
    "notauthenticated",
];

/// A cloud by the launcher's Terraform target for it (`aws`, `azure`, `gcp`, `oci`, …): its name,
/// and how its sign-in is made again.
fn sign_in_of(cloud: &str) -> Option<(&'static str, &'static str)> {
    Some(match cloud {
        "aws" => ("AWS", "`aws login`, or `aws sso login`"),
        "azure" => ("Azure", "`az login`"),
        "gcp" => ("Google Cloud", "`gcloud auth application-default login`"),
        "oci" => ("Oracle Cloud", "`oci session authenticate`, or check `~/.oci/config`"),
        "digitalocean" => ("DigitalOcean", "check the account's API token in Settings"),
        "linode" => ("Linode", "check the account's API token in Settings"),
        _ => return None,
    })
}

/// The cloud a Terraform failure comes from, by its provider's name in the message.
fn cloud_in(reason: &str) -> Option<&'static str> {
    [("hashicorp/aws", "aws"), ("hashicorp/azurerm", "azure"), ("hashicorp/google", "gcp"), ("oracle/oci", "oci")]
        .into_iter()
        .find(|(provider, _)| reason.contains(provider))
        .map(|(_, cloud)| cloud)
}

/// The launcher's word for a cloud tool's `reason` when it says the account's sign-in ran out or
/// is missing (`cloud`: the launcher's Terraform target for it, when known): sign in again,
/// then try again, a refusal before acting (nothing could change without the sign-in); `None`
/// for any other failure.
pub(crate) fn expired_sign_in(reason: &str, cloud: Option<&str>) -> Option<Error> {
    let lower = reason.to_ascii_lowercase();
    if !EXPIRED_SIGN_IN.iter().any(|s| lower.contains(s)) {
        return None;
    }
    let (name, again) = cloud.or_else(|| cloud_in(reason)).and_then(sign_in_of).map_or(("the cloud account", String::new()), |(n, a)| (n, format!(" ({a})")));
    Some(Error::RefusedBeforeActing(format!(
        "Your sign-in to {name} has expired or is missing, so the lab wasn't changed. Sign in again{again}, then try again. ({})",
        reason.trim()
    )))
}

/// A step's tool that ran and failed: Docker not answering its probe reads as the engine being
/// stopped (or this account not allowed to use it); a cloud sign-in that ran out, as signing in
/// again; an address block another network holds, as what to do about it; anything else, as the
/// command that failed.
fn tool_failed(step: &str, reason: &str) -> Error {
    if let Some(expired) = expired_sign_in(reason, None) {
        return expired;
    }
    if step == "tools" && reason.starts_with("`docker` doesn't answer") {
        return if reason.to_ascii_lowercase().contains("permission denied") {
            crate::platform::exec::docker_denied_error()
        } else {
            Error::coded(Code::DockerNotRunning)
        };
    }
    super::super::docker::explain_pool_overlap(Error::CommandFailed { command: format!("lab step {step}"), stderr: reason.to_owned() })
}

/// What the log says before an operation UTM refused in passing is tried once more.
const UTM_TRANSIENT_RETRY: &str =
    "UTM answered \"Application isn't running\" while it runs (a passing UTM scripting error, seen while a VM boots): trying again.";

/// Whether an operation failed on UTM's passing scripting error: `osascript` reports `UTM got an
/// error: Application isn't running. (-600)` from a VM customization right after an import, or
/// from the UTM provider reading the VM's forwarded ports while it boots (a start, a reload),
/// while UTM runs and answers every other command; the same operation then goes through. The
/// other -600, a UTM that really isn't reachable, the start's UTM check catches before.
fn utm_transient(e: &Error) -> bool {
    e.to_string().contains("(-600)")
}

/// Whether `e` is UTM's passing scripting error ([`utm_transient`]), saying in `log` that the
/// operation is tried once more when it is: a start or an update leaves the run as it got it, so
/// the next attempt picks up from there.
pub(crate) fn try_again_after_utm(e: &Error, log: &mut impl FnMut(String)) -> bool {
    let again = utm_transient(e);
    if again {
        log(UTM_TRANSIENT_RETRY.into());
    }
    again
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(e: RunnerError) -> (Code, String) {
        let e = from_runner(e);
        (e.code(), e.to_string())
    }

    #[test]
    fn a_taken_port_and_a_missing_tool_keep_the_codes_the_ui_knows() {
        assert_eq!(said(RunnerError::PortInUse { port: 8080, protocol: "tcp".into() }).0, Code::PortInUse { port: 8080, holder: None });
        assert_eq!(said(RunnerError::ToolMissing { step: "tools".into(), tool: "docker".into() }).0, Code::ToolMissing { tool: "docker".into() });
        assert_eq!(said(RunnerError::Locked { run: "cyberctf-web".into() }).0, Code::AlreadyInProgress);
    }

    #[test]
    fn docker_not_answering_its_probe_reads_as_a_stopped_engine() {
        let down = RunnerError::ToolFailed { step: "tools".into(), reason: "`docker` doesn't answer: Cannot connect to the Docker daemon".into() };
        assert_eq!(said(down).0, Code::DockerNotRunning);
        let denied = RunnerError::ToolFailed { step: "tools".into(), reason: "`docker` doesn't answer: permission denied while trying to connect".into() };
        assert!(matches!(said(denied).0, Code::DockerDenied { .. } | Code::DockerDeniedWindows { .. }));
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
    fn the_lifecycle_refusals_say_what_to_do() {
        let (code, text) = said(RunnerError::AlreadyRunning { run: "cyberctf-web".into() });
        assert_eq!((code, text.as_str()), (Code::Other, "This lab is already running on this machine. Stop it before starting it again."));
        assert!(matches!(from_runner(RunnerError::AlreadyRunning { run: "r".into() }), Error::Refused(_)), "a refusal leaves the last verdict as it was");
        assert!(said(RunnerError::NothingToResume { run: "r".into() }).1.starts_with("Nothing is left of this lab to resume"));
        assert!(said(RunnerError::NotRunning { run: "r".into() }).1.contains("Start it first"));
        let expired = "with provider[\"registry.terraform.io/hashicorp/aws\"] Error: failed to refresh cached credentials, create oauth2 token: login session has expired, please reauthenticate (exited with code 1)";
        let said_expired = said(RunnerError::ToolFailed { step: "destroy".into(), reason: expired.into() }).1;
        assert!(said_expired.starts_with("Your sign-in to AWS has expired") && said_expired.contains("`aws login`"), "{said_expired}");
        for (reason, cloud, again) in [
            ("ERROR: AADSTS700082: The refresh token has expired due to inactivity. Please run 'az login'", "azure", "`az login`"),
            ("Reauthentication failed. cannot prompt during non-interactive execution.", "gcp", "`gcloud auth application-default login`"),
            ("ServiceError: NotAuthenticated: The required information to complete authentication was not provided", "oci", "`oci session authenticate`"),
        ] {
            let said = expired_sign_in(reason, Some(cloud)).expect("an expired sign-in").to_string();
            assert!(said.contains(again), "{said}");
        }
        assert!(expired_sign_in("Error: creating EC2 Instance: InsufficientInstanceCapacity", Some("aws")).is_none());
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
    fn a_vm_labs_own_failures_say_what_to_do() {
        let old = RunnerError::ToolTooOld { tool: "vagrant".into(), found: "2.3.8".into(), required: "2.4.0".into() };
        assert_eq!(said(old).1, "This lab needs Vagrant 2.4.0 or later, and this computer has 2.3.8. Update it, then start the lab again.");
        let declined = RunnerError::Elevation { reason: "the prompt was declined".into() };
        assert!(said(declined).1.starts_with("Hyper-V needs administrator rights"));
        let ceiling = TimedOut::Ceiling { after_s: 21600 };
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
    fn a_manifest_of_another_version_says_to_add_the_lab_again() {
        let source = isoloom_manifest::parse(r#"{"manifest_version": 0}"#).unwrap_err();
        let (code, text) = said(RunnerError::Manifest { path: "run.json".into(), source });
        assert_eq!(code, Code::Other);
        assert!(text.starts_with("This lab's files were made by another version of Cyber CTF, and they couldn't be made again for this one. Remove the lab and add it again from the catalogue"), "{text}");
        assert!(text.contains("(run.json: manifest_version 0"), "the runner's words follow: {text}");
        assert!(!text.contains("Update Cyber CTF"), "updating doesn't help a lab that can't be generated again: {text}");
    }

    #[test]
    fn anything_else_keeps_the_runners_words() {
        let e = RunnerError::Unbound { at: "lifecycle.up.up".into(), placeholder: "params.X".into() };
        let text = e.to_string();
        assert!(said(e).1.ends_with(&text));
    }

    #[test]
    fn a_missing_or_refused_value_changed_nothing() {
        // The runner checks the launch's values before it runs anything: a change refused for one
        // is rolled back like one refused for a run that isn't up.
        for e in [
            RunnerError::SecretMissing { name: "CTF_FLAG_MAIN".into() },
            RunnerError::Param { name: "ISOLOOM_DERIVED".into(), reason: "`2` isn't one of 0, 1".into() },
        ] {
            let launcher = from_runner(e);
            assert!(refused_before_acting(&launcher), "{launcher}");
        }
        let param = from_runner(RunnerError::Param { name: "ISOLOOM_DERIVED".into(), reason: "`2` isn't one of 0, 1".into() });
        assert!(param.to_string().contains("`ISOLOOM_DERIVED`") && param.to_string().contains("`2` isn't one of 0, 1"), "{param}");
        assert!(!refused_before_acting(&Error::Invalid("a step failed".into())));
    }

    #[test]
    fn a_refusal_is_known_by_its_kind_not_its_words() {
        // The run not up, a cloud sign-in that ran out, a busy run, a missing tool, a run up
        // already: refused before acting, whatever they say.
        for refused in [
            from_runner(RunnerError::NotRunning { run: "r".into() }),
            expired_sign_in("Error: login session has expired", Some("aws")).expect("an expired sign-in"),
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
    fn utms_passing_refusal_is_tried_again_and_said_and_nothing_else_is() {
        let mut said = Vec::new();
        let mut log = |l: String| said.push(l);
        let refused = Error::Invalid(
            "`vagrant reload default --no-provision` failed: read_forwarded_ports.applescript: execution error: UTM got an error: Application isn\u{2019}t running. (-600)".into(),
        );
        assert!(try_again_after_utm(&refused, &mut log));
        assert!(!try_again_after_utm(&Error::Invalid("a step failed".into()), &mut log));
        assert_eq!(said, [UTM_TRANSIENT_RETRY]);
    }
}
