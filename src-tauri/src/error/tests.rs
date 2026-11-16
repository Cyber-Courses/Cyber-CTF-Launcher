use super::*;
use serde_json::json;

#[test]
fn errors_reach_the_webview_with_their_code_and_message() {
    let cases = [
        (Error::Invalid("bad id".into()), json!({ "code": "other", "message": "bad id" })),
        (Error::Refused("still running".into()), json!({ "code": "refused", "message": "still running" })),
        (Error::RefusedBeforeActing("not running".into()), json!({ "code": "refusedBeforeActing", "message": "not running" })),
        (Error::Cancelled, json!({ "code": "signInCancelled", "message": "Sign-in cancelled." })),
        (
            Error::Unreachable("API unreachable: timed out".into()),
            json!({ "code": "unreachable", "args": { "detail": "API unreachable: timed out" }, "message": "API unreachable: timed out" }),
        ),
        (
            Error::ToolMissing { tool: "vagrant" },
            json!({ "code": "toolMissing", "args": { "tool": "vagrant" }, "message": "`vagrant` is not installed or not on PATH" }),
        ),
        (
            Error::CommandFailed { command: "docker ps".into(), stderr: "daemon down".into() },
            json!({ "code": "commandFailed", "args": { "command": "docker ps", "stderr": "daemon down" }, "message": "`docker ps` failed: daemon down" }),
        ),
        (Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "no such file")), json!({ "code": "other", "message": "no such file" })),
        (Error::coded(Code::SignedOut), json!({ "code": "signedOut", "message": "You're signed out. Sign in again to continue." })),
        (
            Error::coded(Code::PortInUse { port: crate::runtime::port::HostPort::new(8080), holder: None }),
            json!({ "code": "portInUse", "args": { "port": 8080 }, "message": Code::PortInUse { port: crate::runtime::port::HostPort::new(8080), holder: None }.message() }),
        ),
        (
            // With the holder found, it travels in the args (for the player-facing notice) and
            // in the English message.
            Error::coded(Code::PortInUse { port: crate::runtime::port::HostPort::new(8080), holder: Some("nginx (pid 4123)".into()) }),
            json!({
                "code": "portInUse",
                "args": { "port": 8080, "holder": "nginx (pid 4123)" },
                "message": "Host port 8080 is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again. Held by nginx (pid 4123). Stop it, then start the lab again."
            }),
        ),
    ];
    for (error, wire) in cases {
        assert_eq!(serde_json::to_value(&error).unwrap(), wire);
    }
}

#[test]
fn a_refusal_read_back_from_the_wire_is_still_one() {
    // A deploy worker hands its error back as a Wire: a refusal it read as Other came back as
    // Invalid, neither a refusal nor refused before acting.
    let back = Error::from(Error::Refused("still running on esxi1".into()).to_wire());
    assert!(back.is_refusal() && back.to_string() == "still running on esxi1", "{back:?}");
    let back = Error::from(Error::RefusedBeforeActing("the run isn't up".into()).to_wire());
    assert!(matches!(&back, Error::RefusedBeforeActing(m) if m == "the run isn't up"), "{back:?}");
}

#[test]
fn a_coded_error_says_its_code_in_english() {
    let e = Error::coded(Code::LoginTimeout { minutes: 5 });
    assert_eq!(e.to_string(), "Sign-in wasn't finished within 5 minutes, so it was cancelled. Try again.");
    assert_eq!(e.code(), Code::LoginTimeout { minutes: 5 });
}

/// Each code's English message is the English catalogue's sentence, which every form of every
/// code ([`form`]) has; filled in, no placeholder is left. A plural and a port clash's holder,
/// spelled out.
#[test]
fn every_code_says_its_english_catalogue_sentence() {
    for code in sample_codes().into_iter().filter(|c| !Code::OWN_MESSAGE.contains(c)) {
        let wire = serde_json::to_value(Wire { code: code.clone(), message: String::new() }).unwrap();
        let text = code.message();
        assert_ne!(text, wire["code"].as_str().unwrap(), "no sentence for {wire}");
        assert!(!text.contains('{'), "a placeholder left in {wire}: {text}");
    }
    assert_eq!(Code::HostHasLabs { count: 1 }.message(), "1 lab still running on this host. Stop it before removing the host.");
    assert_eq!(Code::HostHasLabs { count: 3 }.message(), "3 labs still running on this host. Stop them before removing the host.");
    assert_eq!(
        Code::PortInUse { port: crate::runtime::port::HostPort::new(8000), holder: Some("nginx (pid 4123)".into()) }.message(),
        "Host port 8000 is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again. \
             Held by nginx (pid 4123). Stop it, then start the lab again."
    );
}

/// Which form of a code `code` is: its variant, a port clash told with or without its holder.
/// No wildcard: a new variant doesn't compile here until it has a form, and then
/// [`every_form_has_a_sample`] fails until [`sample_codes`] has one.
fn form(code: &Code) -> usize {
    match code {
        Code::Other => 0,
        Code::SignedOut => 1,
        Code::SessionExpired => 2,
        Code::LoginTimeout { .. } => 3,
        Code::AlreadyInProgress => 4,
        Code::NoPasswordStored => 5,
        Code::PortInUse { holder: None, .. } => 6,
        Code::PortInUse { holder: Some(_), .. } => 7,
        Code::SignInCancelled => 8,
        Code::SignInDenied => 9,
        Code::SignInCodeUnavailable => 10,
        Code::Unreachable { .. } => 11,
        Code::OfflineReady { .. } => 12,
        Code::DockerNotRunning => 13,
        Code::DockerDenied { .. } => 14,
        Code::DockerDeniedWindows { .. } => 15,
        Code::ReapNoHost => 16,
        Code::PlacementLockedHost { .. } => 17,
        Code::PlacementLockedAccount { .. } => 18,
        Code::LabRunningHere => 19,
        Code::LabRunningOn { .. } => 20,
        Code::HostHasLabs { .. } => 21,
        Code::ToolMissing { .. } => 22,
        Code::CommandFailed { .. } => 23,
        Code::TurnedOffByPolicy { .. } => 24,
        Code::Refused => 25,
        Code::RefusedBeforeActing => 26,
        Code::PickCloudAccount => 27,
        Code::NoHypervisor => 28,
        Code::DownloadStalled { .. } => 29,
        Code::DownloadFailed { .. } => 30,
        Code::BoxDownloadStalled { .. } => 31,
        Code::BoxDownloadFailed { .. } => 32,
        Code::ServerAuthFailed { .. } => 33,
        Code::ToolTooOld { .. } => 34,
        Code::ElevationFailed { .. } => 35,
        Code::LabNotReady { .. } => 36,
        Code::MachinesGone { .. } => 37,
        Code::NothingToResume => 38,
        Code::NotParkable => 39,
        Code::StepStalled { .. } => 40,
        Code::StepSilent { .. } => 41,
        Code::StepCeiling { .. } => 42,
        Code::StepTimedOut { .. } => 43,
        Code::TlsUnconfirmed { .. } => 44,
        Code::TlsChanged { .. } => 45,
        Code::SshHostKeyRefused { .. } => 46,
        Code::VirtiofsdMissing => 47,
    }
}

/// How many forms [`form`] tells apart.
const FORMS: usize = 48;

#[test]
fn every_form_has_a_sample() {
    let covered: std::collections::BTreeSet<usize> = sample_codes().iter().map(form).collect();
    assert_eq!(covered, (0..FORMS).collect(), "a form of a code without a sample in sample_codes");
}

fn sample_codes() -> Vec<Code> {
    vec![
        Code::Other,
        Code::Refused,
        Code::RefusedBeforeActing,
        Code::SignedOut,
        Code::SessionExpired,
        Code::LoginTimeout { minutes: 5 },
        Code::SignInCancelled,
        Code::SignInDenied,
        Code::SignInCodeUnavailable,
        Code::Unreachable { detail: "API unreachable: timed out".into() },
        Code::OfflineReady { detail: "API unreachable: timed out".into() },
        Code::AlreadyInProgress,
        Code::NoPasswordStored,
        Code::PortInUse { port: crate::runtime::port::HostPort::new(443), holder: None },
        Code::PortInUse { port: crate::runtime::port::HostPort::new(8000), holder: Some("nginx (pid 4123)".into()) },
        Code::DockerNotRunning,
        Code::DockerDenied { user: "player".into() },
        Code::DockerDeniedWindows { user: "Ada Lovelace".into() },
        Code::PlacementLockedHost { count: 1, fields: "node, host".into() },
        Code::PlacementLockedAccount { count: 2, fields: "region".into() },
        Code::HostHasLabs { count: 1 },
        Code::HostHasLabs { count: 3 },
        Code::LabRunningHere,
        Code::LabRunningOn { host: "My Proxmox".into() },
        Code::ReapNoHost,
        Code::ToolMissing { tool: "terraform".into() },
        Code::CommandFailed { command: "vagrant up".into(), stderr: "locked".into() },
        Code::DownloadStalled { minutes: 15 },
        Code::DownloadFailed { reason: "toomanyrequests: You have reached your pull rate limit".into() },
        Code::BoxDownloadStalled { minutes: 15 },
        Code::BoxDownloadFailed { reason: "The box 'bento/debian-12' could not be found".into() },
        Code::ServerAuthFailed { server: "Home lab Proxmox".into() },
        Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() },
        Code::PickCloudAccount,
        Code::NoHypervisor,
        Code::VirtiofsdMissing,
        Code::ToolTooOld { tool: "Vagrant".into(), found: "2.3.4".into(), required: "2.4.0".into() },
        Code::ElevationFailed { reason: "the permission prompt was declined".into() },
        Code::LabNotReady { detail: "http://web:80 answered 502".into() },
        Code::MachinesGone { machines: "web, db".into() },
        Code::NothingToResume,
        Code::NotParkable,
        Code::StepStalled { step: "up".into(), minutes: 30, last_line: "web Downloading 1.1GB".into() },
        Code::StepSilent { step: "up".into(), minutes: 30 },
        Code::StepCeiling { step: "up".into(), hours: 6 },
        Code::StepTimedOut { step: "up".into(), minutes: 30 },
        Code::TlsUnconfirmed { server: "Garage Proxmox".into(), presented: "4F:1F:C1".into() },
        Code::TlsChanged { server: "Garage Proxmox".into(), pinned: "4F:1F:C1".into(), presented: "0A:2B:3C".into() },
        Code::SshHostKeyRefused { host: "10.0.0.50".into() },
    ]
}

#[test]
fn every_code_round_trips_through_the_wire() {
    for code in sample_codes() {
        // As the core raises it: its own variant where it has one, else coded.
        let error = match code.clone() {
            Code::Other => Error::Invalid("bad id".into()),
            Code::Refused => Error::Refused("still running elsewhere".into()),
            Code::RefusedBeforeActing => Error::RefusedBeforeActing("the run isn't up".into()),
            Code::SignInCancelled => Error::Cancelled,
            Code::Unreachable { detail } => Error::Unreachable(detail),
            c => Error::coded(c),
        };
        let wire = error.to_wire();
        assert_eq!(wire.code, code);
        let text = serde_json::to_string(&wire).unwrap();
        let back: Wire = serde_json::from_str(&text).unwrap();
        assert_eq!(back, wire, "{text}");
        let error = Error::from(back);
        assert_eq!(error.to_wire(), wire, "{text}");
    }
}
