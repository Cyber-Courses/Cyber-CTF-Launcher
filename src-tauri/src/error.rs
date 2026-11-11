//! The launcher's errors: [`Error`] inside the app, [`Wire`] as the webview gets it, and the
//! [`Code`] the webview branches on and says in the player's language.

use serde::{Deserialize, Serialize};

/// What went wrong, as the UI knows it: a code it can branch on and say in the player's
/// language, with the values its message needs. `Other` and the refusals are the codes the UI
/// shows as the error's own (English) message ([`Code::OWN_MESSAGE`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "code", content = "args", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "ErrorCode")]
pub enum Code {
    /// Anything the UI has no code for: it shows the error's own (English) message.
    Other,
    /// An operation refused before it changed anything ([`Error::Refused`]), in its own words.
    Refused,
    /// A change of a lab refused before it acted, for something to put right first
    /// ([`Error::RefusedBeforeActing`]), in its own words.
    RefusedBeforeActing,
    /// A call that needs the account, with no session kept.
    SignedOut,
    /// The session can't be renewed any more: signed out.
    SessionExpired,
    /// The browser sign-in wasn't finished in time.
    LoginTimeout {
        /// How long the sign-in waited, in minutes.
        minutes: u64,
    },
    /// Another operation on the lab runs: this one was refused before it changed anything.
    AlreadyInProgress,
    /// A server host saved without its password.
    NoPasswordStored,
    /// A host port is taken: one a container lab publishes, or one a local VM forwards. `holder`
    /// is a short description of what holds it ("nginx (pid 4123)"), filled in on the failure
    /// path when it can be found, so the player knows what to stop.
    PortInUse {
        /// The host port that is taken.
        port: u16,
        /// What holds the port, when it could be found.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<String>,
    },
    /// The player cancelled the sign-in (or started another one): not a failure to show.
    SignInCancelled,
    /// The player declined the sign-in at cyber-auth (its consent screen, or the code's page).
    SignInDenied,
    /// cyber-auth doesn't let the launcher sign in with a code (the device grant isn't enabled
    /// on its OAuth client): the browser sign-in still works.
    SignInCodeUnavailable,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an
    /// answer that isn't the API's): trying again later may work. `detail` is the technical why.
    Unreachable {
        /// The technical why: the transport's error or what answered instead.
        detail: String,
    },
    /// Cyber CTF couldn't be reached, and the lab is installed here, so it can start offline.
    OfflineReady {
        /// The technical why, as for `Unreachable`.
        detail: String,
    },
    /// The container engine isn't running.
    DockerNotRunning,
    /// Docker runs, but this account isn't allowed to use it.
    DockerDenied {
        /// The account's user name, for the `usermod` command the message gives.
        user: String,
    },
    /// The same on Windows: the engine's pipe belongs to the `docker-users` group.
    DockerDeniedWindows {
        /// The account's user name, for the `net localgroup` command the message gives.
        user: String,
    },
    /// The auto-stop couldn't destroy a lab: no account is recorded for its run. (Its other
    /// failures are the runner's, with their own codes.)
    ReapNoHost,
    /// A server host's placement (provider, address, node…) can't change while labs are on it.
    PlacementLockedHost {
        /// How many labs are on the host.
        count: u64,
        /// The placement fields the edit would change, as the message lists them.
        fields: String,
    },
    /// The same for a cloud account (its region, subscription, profile…).
    PlacementLockedAccount {
        /// How many labs are on the account.
        count: u64,
        /// The placement fields the edit would change, as the message lists them.
        fields: String,
    },
    /// Something of the lab is still up on this machine (running, or its machines left over): it
    /// can't be replaced, nor started somewhere else, until it is stopped.
    LabRunningHere,
    /// The same on the saved server or cloud account `host` (its name).
    LabRunningOn {
        /// The saved host's name.
        host: String,
    },
    /// A host can't be removed while labs run on it.
    HostHasLabs {
        /// How many labs run on the host.
        count: u64,
    },
    /// A tool the operation runs isn't installed.
    ToolMissing {
        /// The tool's command name (`docker`, `vagrant`, …).
        tool: String,
    },
    /// A command ran and failed.
    CommandFailed {
        /// The command line that failed.
        command: String,
        /// What the command wrote to its standard error.
        stderr: String,
    },
    /// A lab that runs only in a cloud account was started with none picked.
    PickCloudAccount,
    /// A VM lab was started on this machine with no hypervisor here to run it on.
    NoHypervisor,
    /// This computer's administrator turned the feature off: `setting` is the policy switch
    /// (`platform::policy::Key`) that did.
    TurnedOffByPolicy {
        /// The policy setting's name.
        setting: String,
    },
}

impl Code {
    /// The codes whose message is the error's own English text rather than a sentence of the
    /// webview's catalogue: anything without a code, and the refusals, which say in their own
    /// words what to put right. The webview reads this list from the generated bindings, which the
    /// tests write (`testing::ts_bindings`).
    #[cfg(test)]
    pub const OWN_MESSAGE: [Code; 3] = [Code::Other, Code::Refused, Code::RefusedBeforeActing];

    /// The English message, for the log and for a UI without this code's translation; for a code
    /// of [`Code::OWN_MESSAGE`], what is said when an error has none of its own.
    fn message(&self) -> String {
        match self {
            Code::Other => "Something went wrong.".into(),
            Code::Refused => "The operation was refused before it changed anything.".into(),
            Code::RefusedBeforeActing => "The change was refused before it acted: nothing changed.".into(),
            Code::SignedOut => "You're signed out. Sign in again to continue.".into(),
            Code::SessionExpired => "Your session expired: you're signed out. Sign in again to continue.".into(),
            Code::LoginTimeout { minutes } => format!("Sign-in wasn't finished within {minutes} minutes, so it was cancelled. Try again."),
            Code::AlreadyInProgress => "Another operation on this lab is already in progress; wait for it to finish.".into(),
            Code::NoPasswordStored => "No password is stored for this host. Edit it and enter one.".into(),
            Code::SignInCancelled => "Sign-in cancelled.".into(),
            Code::SignInDenied => "Sign-in was declined at Cyber Auth, so you're still signed out. Sign in again and allow Cyber CTF to continue.".into(),
            Code::SignInCodeUnavailable => {
                "Signing in with a code isn't available yet. Copy the sign-in link into any browser instead, on this machine or another.".into()
            }
            Code::Unreachable { detail } => format!("Cyber CTF can't be reached ({detail}). Check your connection and try again."),
            Code::OfflineReady { detail } => {
                format!("Cyber CTF can't be reached ({detail}). This lab is installed here, so it can start offline, without saving progress.")
            }
            Code::PortInUse { port, holder } => {
                let clash = format!(
                    "Host port {port} is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again."
                );
                // What holds it, as the lab page says it under the error (`labs.failure.portHeldBy`):
                // the catalogue has one form, the holder its own line.
                match holder {
                    Some(holder) => format!("{clash} Held by {holder}. Stop it, then start the lab again."),
                    None => clash,
                }
            }
            Code::DockerNotRunning => {
                "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.".into()
            }
            Code::DockerDenied { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker {user}), then log out and back in."
            ),
            Code::DockerDeniedWindows { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker-users group (in a terminal run as administrator: net localgroup docker-users \"{user}\" /add), then sign out and back in."
            ),
            Code::ReapNoHost => "No account is recorded for it, so the launcher can't destroy it. Remove its resources in your cloud console.".into(),
            Code::PlacementLockedHost { count, fields } => placement_locked(*count, "host", fields),
            Code::PlacementLockedAccount { count, fields } => placement_locked(*count, "account", fields),
            Code::LabRunningHere => "This lab is still running on this machine (or its machines are still there). Stop it, then try again.".into(),
            Code::LabRunningOn { host } => format!("This lab is still running on {host}. Stop it there, then try again."),
            Code::HostHasLabs { count } => {
                let (labs, them) = if *count == 1 { ("lab", "it") } else { ("labs", "them") };
                format!("{count} {labs} still running on this host. Stop {them} before removing the host.")
            }
            Code::ToolMissing { tool } => format!("`{tool}` is not installed or not on PATH"),
            Code::CommandFailed { command, stderr } => format!("`{command}` failed: {stderr}"),
            Code::PickCloudAccount => "This lab runs in your cloud account: pick one of your cloud accounts to start it on.".into(),
            Code::NoHypervisor => {
                "No hypervisor ready on this machine runs this lab. Install one from the Machine page, or run the lab on a server or in your cloud account."
                    .into()
            }
            Code::TurnedOffByPolicy { setting } => {
                format!("Your organization turned this off on this computer (policy setting {setting}). Ask your IT team if you need it.")
            }
        }
    }
}

/// "2 labs are still on this account: stop them before changing its region."
fn placement_locked(count: u64, what: &str, fields: &str) -> String {
    let (labs, them) = if count == 1 { ("lab is", "it") } else { ("labs are", "them") };
    format!("{count} {labs} still on this {what}: stop {them} before changing its {fields}.")
}

/// A failure of the launcher's own code, as its functions return it; the webview gets it as a
/// [`Wire`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A request or a value the operation can't use (a bad argument, a file that isn't what it
    /// should be): its message says what.
    #[error("{0}")]
    Invalid(String),
    /// An operation refused before it changed anything (a lab still up elsewhere, an update of a
    /// running lab): not a failed deploy, so the lab's last verdict stays what it was.
    #[error("{0}")]
    Refused(String),
    /// A change of a lab refused before it acted, for something to put right first (the run isn't
    /// up, a value it needs is missing or unusable, a cloud sign-in ran out): nothing changed, so
    /// what was written for it can be put back. Unlike [`Error::Refused`], a launch refused so
    /// failed: its verdict says so.
    #[error("{0}")]
    RefusedBeforeActing(String),
    /// The player cancelled sign-in (or started another one): not a failure to show.
    #[error("{}", Code::SignInCancelled.message())]
    Cancelled,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an answer
    /// that isn't the API's): unlike a refusal, trying again later may work.
    #[error("{0}")]
    Unreachable(String),
    /// A failure the UI knows by its code (see [`Code`]).
    #[error("{message}")]
    Coded {
        /// The code the UI branches on.
        code: Code,
        /// The code's English message.
        message: String,
    },
    /// A tool the operation runs isn't installed, or isn't on `PATH`.
    #[error("{}", Code::ToolMissing { tool: (*tool).into() }.message())]
    ToolMissing {
        /// The tool's command name.
        tool: &'static str,
    },
    /// A command ran and failed.
    #[error("{}", Code::CommandFailed { command: command.clone(), stderr: stderr.clone() }.message())]
    CommandFailed {
        /// The command line that failed.
        command: String,
        /// What the command wrote to its standard error.
        stderr: String,
    },
    /// A lab's lock couldn't be taken (its file can't be opened or locked): the operation stops
    /// there rather than run unlocked.
    #[error("Couldn't lock the lab: {0}")]
    LockUnavailable(#[source] std::io::Error),
    /// An I/O failure, with the system's message.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// The error with this code, and the code's English message.
    pub fn coded(code: Code) -> Self {
        Error::Coded { message: code.message(), code }
    }

    /// Whether the operation was refused before it changed anything (the lab still up where it
    /// would be replaced or started again, a policy that turns it off, a start with nowhere to
    /// run): the lab's last verdict stays what it was, and a failed start has nothing of its own
    /// to undo.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Error::Refused(_)
                | Error::Coded {
                    code: Code::LabRunningHere | Code::LabRunningOn { .. } | Code::TurnedOffByPolicy { .. } | Code::PickCloudAccount | Code::NoHypervisor,
                    ..
                }
        )
    }

    /// The code the UI knows this error by.
    pub fn code(&self) -> Code {
        match self {
            Error::Coded { code, .. } => code.clone(),
            Error::ToolMissing { tool } => Code::ToolMissing { tool: (*tool).into() },
            Error::CommandFailed { command, stderr } => Code::CommandFailed { command: command.clone(), stderr: stderr.clone() },
            Error::Cancelled => Code::SignInCancelled,
            Error::Unreachable(detail) => Code::Unreachable { detail: detail.clone() },
            Error::Refused(_) => Code::Refused,
            Error::RefusedBeforeActing(_) => Code::RefusedBeforeActing,
            Error::Invalid(_) | Error::LockUnavailable(_) | Error::Io(_) => Code::Other,
        }
    }

    /// The error as the webview gets it (and as a deploy worker hands it back to the app).
    pub fn to_wire(&self) -> Wire {
        Wire { code: self.code(), message: self.to_string() }
    }
}

/// An error as the webview receives it: `{ code, args?, message }`. `message` is English and
/// always there, for the log and for the codes the UI says as they come ([`Code::OWN_MESSAGE`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "CommandError")]
pub struct Wire {
    /// The code the UI branches on, with its values (`code` and `args` on the wire).
    #[serde(flatten)]
    pub code: Code,
    /// The English message.
    pub message: String,
}

impl From<Wire> for Error {
    fn from(w: Wire) -> Self {
        match w.code {
            Code::Other => Error::Invalid(w.message),
            Code::Refused => Error::Refused(w.message),
            Code::RefusedBeforeActing => Error::RefusedBeforeActing(w.message),
            Code::SignInCancelled => Error::Cancelled,
            Code::Unreachable { .. } => Error::Unreachable(w.message),
            code => Error::Coded { code, message: w.message },
        }
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.to_wire().serialize(s)
    }
}

/// The launcher's result: an [`Error`] on failure.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
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
                Error::coded(Code::PortInUse { port: 8080, holder: None }),
                json!({ "code": "portInUse", "args": { "port": 8080 }, "message": Code::PortInUse { port: 8080, holder: None }.message() }),
            ),
            (
                // With the holder found, it travels in the args (for the player-facing notice) and
                // in the English message.
                Error::coded(Code::PortInUse { port: 8080, holder: Some("nginx (pid 4123)".into()) }),
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

    /// Each code's English message is said twice: here (the log, a worker's ✗ line) and in the
    /// webview's catalog (`errors.codes.*`, what the English UI shows). They must agree, for every
    /// form of every code ([`form`]). A port clash with its holder is the catalogue's sentence,
    /// then the holder's line the lab page shows under it (`labs.failure.portHeldBy`): the
    /// catalogue has no holder form of its own.
    #[test]
    fn every_code_says_what_the_english_catalog_says() {
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../../src/messages/en/errors.json")).unwrap();
        let labs: serde_json::Value = serde_json::from_str(include_str!("../../src/messages/en/labs.json")).unwrap();
        for code in sample_codes().into_iter().filter(|c| !Code::OWN_MESSAGE.contains(c)) {
            let wire = serde_json::to_value(Wire { code: code.clone(), message: String::new() }).unwrap();
            let entry = &catalog["codes"][wire["code"].as_str().unwrap()];
            // A plural ({ one, other }) by its `count`, as the webview picks it in English.
            let form = if wire["args"]["count"] == 1 { "one" } else { "other" };
            let mut text = entry.as_str().or_else(|| entry[form].as_str()).unwrap_or_else(|| panic!("no message for {wire}")).to_string();
            for (name, value) in wire["args"].as_object().into_iter().flatten() {
                let value = value.as_str().map_or_else(|| value.to_string(), str::to_string);
                text = text.replace(&format!("{{{name}}}"), &value);
            }
            if let Code::PortInUse { holder: Some(holder), .. } = &code {
                let held = labs["failure"]["portHeldBy"].as_str().unwrap().replace("{holder}", holder);
                text = format!("{text} {held}");
            }
            assert_eq!(code.message(), text, "{wire}");
        }
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
        }
    }

    /// How many forms [`form`] tells apart.
    const FORMS: usize = 29;

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
            Code::PortInUse { port: 443, holder: None },
            Code::PortInUse { port: 8000, holder: Some("nginx (pid 4123)".into()) },
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
            Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() },
            Code::PickCloudAccount,
            Code::NoHypervisor,
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
}
