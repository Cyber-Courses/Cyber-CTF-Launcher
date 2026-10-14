use serde::{Deserialize, Serialize};

/// What went wrong, as the UI knows it: a code it can branch on and say in the player's
/// language, with the values its message needs. `Other` is everything the UI shows as the
/// error's own (English) message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "code", content = "args", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "ErrorCode")]
pub enum Code {
    Other,
    /// A call that needs the account, with no session kept.
    SignedOut,
    /// The session can't be renewed any more: signed out.
    SessionExpired,
    /// The browser sign-in wasn't finished in time.
    LoginTimeout {
        minutes: u64,
    },
    /// No browser could be opened for the sign-in.
    NoBrowser,
    /// Another operation on the lab runs: this one was refused before it changed anything.
    AlreadyInProgress,
    /// A server host saved without its password.
    NoPasswordStored,
    /// A host port a container lab publishes is taken.
    PortInUse {
        port: u16,
    },
    /// The player cancelled the sign-in (or started another one): not a failure to show.
    SignInCancelled,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an
    /// answer that isn't the API's): trying again later may work. `detail` is the technical why.
    Unreachable {
        detail: String,
    },
    /// Cyber CTF couldn't be reached, and the lab is installed here, so it can start offline.
    OfflineReady {
        detail: String,
    },
    /// The container engine isn't running.
    DockerNotRunning,
    /// Docker runs, but this account isn't allowed to use it.
    DockerDenied {
        user: String,
    },
    /// A tool the operation runs isn't installed.
    ToolMissing {
        tool: String,
    },
    /// A command ran and failed.
    CommandFailed {
        command: String,
        stderr: String,
    },
}

impl Code {
    /// The English message, for the log and for a UI without this code's translation.
    fn message(&self) -> String {
        match self {
            Code::Other => "Something went wrong.".into(),
            Code::SignedOut => "You're signed out. Sign in again to continue.".into(),
            Code::SessionExpired => "Your session expired: you're signed out. Sign in again to continue.".into(),
            Code::LoginTimeout { minutes } => format!("Sign-in wasn't finished within {minutes} minutes, so it was cancelled. Try again."),
            Code::NoBrowser => "No browser opened for sign-in. Set a default web browser for this account and try again.".into(),
            Code::AlreadyInProgress => "Another operation on this lab is already in progress; wait for it to finish.".into(),
            Code::NoPasswordStored => "No password is stored for this host. Edit it and enter one.".into(),
            Code::SignInCancelled => "Sign-in cancelled.".into(),
            Code::Unreachable { detail } => format!("Cyber CTF can't be reached ({detail}). Check your connection and try again."),
            Code::OfflineReady { detail } => {
                format!("Cyber CTF can't be reached ({detail}). This lab is installed here, so it can start offline, without saving progress.")
            }
            Code::PortInUse { port } => format!(
                "Host port {port} is already in use on this machine (another lab or app holds it). Stop whatever uses it, or start the lab with random ports."
            ),
            Code::DockerNotRunning => {
                "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.".into()
            }
            Code::DockerDenied { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker {user}), then log out and back in."
            ),
            Code::DockerDeniedWindows { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker-users group (in a terminal run as administrator: net localgroup docker-users \"{user}\" /add), then sign out and back in."
            ),
            Code::ToolMissing { tool } => format!("`{tool}` is not installed or not on PATH"),
            Code::CommandFailed { command, stderr } => format!("`{command}` failed: {stderr}"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    /// An operation refused before it changed anything (a lab still up elsewhere, an update of a
    /// running lab): not a failed deploy, so the lab's last verdict stays what it was.
    #[error("{0}")]
    Refused(String),
    /// The player cancelled sign-in (or started another one): not a failure to show.
    #[error("Sign-in cancelled.")]
    Cancelled,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an answer
    /// that isn't the API's): unlike a refusal, trying again later may work.
    #[error("{0}")]
    Unreachable(String),
    /// A failure the UI knows by its code (see [`Code`]).
    #[error("{message}")]
    Coded { code: Code, message: String },
    #[error("`{tool}` is not installed or not on PATH")]
    ToolMissing { tool: &'static str },
    #[error("`{command}` failed: {stderr}")]
    CommandFailed { command: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// The error with this code, and the code's English message.
    pub fn coded(code: Code) -> Self {
        Error::Coded { message: code.message(), code }
    }

    /// The code the UI knows this error by.
    pub fn code(&self) -> Code {
        match self {
            Error::Coded { code, .. } => code.clone(),
            Error::ToolMissing { tool } => Code::ToolMissing { tool: (*tool).into() },
            Error::CommandFailed { command, stderr } => Code::CommandFailed { command: command.clone(), stderr: stderr.clone() },
            Error::Cancelled => Code::SignInCancelled,
            Error::Unreachable(detail) => Code::Unreachable { detail: detail.clone() },
            Error::Invalid(_) | Error::Refused(_) | Error::Io(_) => Code::Other,
        }
    }

    /// The error as the webview gets it (and as a deploy worker hands it back to the app).
    pub fn to_wire(&self) -> Wire {
        Wire { code: self.code(), message: self.to_string() }
    }
}

/// An error as the webview receives it: `{ code, args?, message }`. `message` is English and
/// always there, for the log and for codes the UI doesn't translate (`other`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "CommandError")]
pub struct Wire {
    #[serde(flatten)]
    pub code: Code,
    pub message: String,
}

impl From<Wire> for Error {
    fn from(w: Wire) -> Self {
        match w.code {
            Code::Other => Error::Invalid(w.message),
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

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn errors_reach_the_webview_with_their_code_and_message() {
        let cases = [
            (Error::Invalid("bad id".into()), json!({ "code": "other", "message": "bad id" })),
            (Error::Refused("still running".into()), json!({ "code": "other", "message": "still running" })),
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
                Error::coded(Code::PortInUse { port: 8080 }),
                json!({ "code": "portInUse", "args": { "port": 8080 }, "message": Code::PortInUse { port: 8080 }.message() }),
            ),
        ];
        for (error, wire) in cases {
            assert_eq!(serde_json::to_value(&error).unwrap(), wire);
        }
    }

    #[test]
    fn a_coded_error_says_its_code_in_english() {
        let e = Error::coded(Code::LoginTimeout { minutes: 5 });
        assert_eq!(e.to_string(), "Sign-in wasn't finished within 5 minutes, so it was cancelled. Try again.");
        assert_eq!(e.code(), Code::LoginTimeout { minutes: 5 });
    }

    /// Each code's English message is said twice: here (the log, a worker's ✗ line) and in the
    /// webview's catalog (`errors.codes.*`, what the English UI shows). They must agree.
    #[test]
    fn every_code_says_what_the_english_catalog_says() {
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../../src/messages/en/errors.json")).unwrap();
        for code in sample_codes().into_iter().filter(|c| *c != Code::Other) {
            let wire = serde_json::to_value(Wire { code: code.clone(), message: String::new() }).unwrap();
            let mut text = catalog["codes"][wire["code"].as_str().unwrap()].as_str().unwrap_or_else(|| panic!("no message for {wire}")).to_string();
            for (name, value) in wire["args"].as_object().into_iter().flatten() {
                let value = value.as_str().map_or_else(|| value.to_string(), str::to_string);
                text = text.replace(&format!("{{{name}}}"), &value);
            }
            assert_eq!(code.message(), text, "{wire}");
        }
    }

    fn sample_codes() -> Vec<Code> {
        vec![
            Code::Other,
            Code::SignedOut,
            Code::SessionExpired,
            Code::LoginTimeout { minutes: 5 },
            Code::NoBrowser,
            Code::SignInCancelled,
            Code::Unreachable { detail: "API unreachable: timed out".into() },
            Code::OfflineReady { detail: "API unreachable: timed out".into() },
            Code::AlreadyInProgress,
            Code::NoPasswordStored,
            Code::PortInUse { port: 443 },
            Code::DockerNotRunning,
            Code::DockerDenied { user: "player".into() },
            Code::DockerDeniedWindows { user: "Ada Lovelace".into() },
            Code::ToolMissing { tool: "terraform".into() },
            Code::CommandFailed { command: "vagrant up".into(), stderr: "locked".into() },
        ]
    }

    #[test]
    fn every_code_round_trips_through_the_wire() {
        for code in sample_codes() {
            // As the core raises it: its own variant where it has one, else coded.
            let error = match code.clone() {
                Code::Other => Error::Invalid("bad id".into()),
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
