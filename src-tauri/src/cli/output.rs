//! What the command line prints, and how it exits. Text for a person by default; with `--json`,
//! one JSON object a line on stdout, each with the shape's version (`v`), its type (`event`,
//! `result` or `error`) and the command (`labs.start`): the events as the command goes, then its
//! result or its error. The shape is versioned: a field is added within a version, never renamed,
//! retyped or removed (docs/cli.md).

use std::io::Write;

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Code, Error, Wire};

/// The JSON shape's version.
pub const SHAPE: u32 = 1;

/// How the command line exits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It did what it was asked.
    Ok = 0,
    /// The operation failed.
    Failed = 1,
    /// The command line was wrong: an unknown command or option, a bad value, a destructive
    /// command without `--yes` off a terminal.
    Usage = 2,
    /// Something must be put right first: the lab isn't installed, another operation runs on it,
    /// Docker isn't running, the account is signed out, elevation is impossible, a restart is due.
    NotReady = 3,
}

impl Exit {
    /// The process's exit code.
    pub fn code(self) -> u8 {
        self as u8
    }

    /// How a failure with `code` exits: a precondition to put right first is [`Exit::NotReady`],
    /// anything else [`Exit::Failed`].
    pub fn of(code: &Code) -> Exit {
        match code {
            Code::Refused
            | Code::RefusedBeforeActing
            | Code::SignedOut
            | Code::SessionExpired
            | Code::AlreadyInProgress
            | Code::NoPasswordStored
            | Code::OfflineReady { .. }
            | Code::DockerNotRunning
            | Code::DockerDenied { .. }
            | Code::DockerDeniedWindows { .. }
            | Code::LabRunningHere
            | Code::LabRunningOn { .. }
            | Code::HostHasLabs { .. }
            | Code::ToolMissing { .. } => Exit::NotReady,
            _ => Exit::Failed,
        }
    }
}

/// A command's failure: how it exits, the error, and the operation it was, when one.
#[derive(Debug)]
pub struct Failure {
    /// How the command line exits.
    pub exit: Exit,
    /// What went wrong, as the app's error says it. Boxed: every command's result carries a
    /// `Failure` on its error side, which stays small.
    pub error: Box<Wire>,
    /// The operation's id, for the player to quote.
    pub operation: Option<String>,
}

impl Failure {
    /// A usage error saying `message`.
    pub fn usage(message: impl Into<String>) -> Failure {
        Failure { exit: Exit::Usage, error: Box::new(Error::Invalid(message.into()).to_wire()), operation: None }
    }

    /// Something to put right first, said by `message`.
    pub fn not_ready(message: impl Into<String>) -> Failure {
        Failure { exit: Exit::NotReady, error: Box::new(Error::Refused(message.into()).to_wire()), operation: None }
    }

    /// The failure of operation `operation` with `error`.
    pub fn of_operation(error: Wire, operation: Option<String>) -> Failure {
        Failure { exit: Exit::of(&error.code), error: Box::new(error), operation }
    }
}

impl From<Error> for Failure {
    /// The app's error, exiting as its code says ([`Exit::of`]); a lab that isn't installed is
    /// something to put right first.
    fn from(e: Error) -> Failure {
        let error = e.to_wire();
        let exit = if error.message.ends_with("is not installed") {
            Exit::NotReady
        } else if error.code == Code::Other && error.message.starts_with("invalid ") {
            // The app's word for a value refused as given (an id, a machine name, an image).
            Exit::Usage
        } else {
            Exit::of(&error.code)
        };
        Failure { exit, error: Box::new(error), operation: None }
    }
}

/// What a command answers: its result, or why it failed.
pub type Answer = std::result::Result<Value, Failure>;

/// Where the command line writes: stdout and stderr (a test's buffers), in text or JSON.
pub struct Output<'a> {
    /// One JSON object a line, instead of text.
    pub json: bool,
    /// The command, as the JSON lines name it (`labs.start`).
    pub command: String,
    /// Where results, events and text go.
    stdout: &'a mut dyn Write,
    /// Where text errors and prompts go.
    stderr: &'a mut dyn Write,
}

impl<'a> Output<'a> {
    /// Output into `stdout` and `stderr`, as JSON lines when `json`.
    pub fn new(json: bool, stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Output<'a> {
        Output { json, command: String::new(), stdout, stderr }
    }

    /// One JSON line of `kind`, with `fields`.
    fn json_line(&mut self, kind: &str, fields: Value) {
        let mut line = json!({ "v": SHAPE, "type": kind, "command": self.command });
        if let (Some(line), Value::Object(fields)) = (line.as_object_mut(), fields) {
            line.extend(fields);
        }
        // A closed stdout (`| head`) ends nothing: the command still runs to its end.
        let _ = writeln!(self.stdout, "{line}");
    }

    /// A line of text for the person reading (nothing in JSON).
    pub fn say(&mut self, text: &str) {
        if !self.json {
            let _ = writeln!(self.stdout, "{text}");
        }
    }

    /// A progress event: `event` as a JSON line, or `text` (when there is any) for a person.
    pub fn event(&mut self, event: &impl Serialize, text: Option<&str>) {
        if self.json {
            self.json_line("event", json!({ "event": event }));
        } else if let Some(text) = text {
            let _ = writeln!(self.stdout, "{text}");
        }
    }

    /// A prompt or a note on stderr, never in the JSON stream.
    pub fn note(&mut self, text: &str) {
        let _ = writeln!(self.stderr, "{text}");
        let _ = self.stderr.flush();
    }

    /// The command's end: its result (`data`, with `text` its readable form), or its failure.
    /// Returns how the command line exits.
    pub fn finish(&mut self, answer: Answer, text: impl FnOnce(&Value) -> String) -> Exit {
        match answer {
            Ok(data) => {
                if self.json {
                    self.json_line("result", json!({ "data": data }));
                } else {
                    let text = text(&data);
                    if !text.is_empty() {
                        let _ = writeln!(self.stdout, "{}", text.trim_end());
                    }
                }
                Exit::Ok
            }
            Err(failure) => self.fail(&failure),
        }
    }

    /// The command's failure, said. Returns how the command line exits.
    pub fn fail(&mut self, failure: &Failure) -> Exit {
        if self.json {
            // The app's error as the webview gets it (`code`, `args` when it has values, `message`),
            // with the operation's id.
            let mut error = serde_json::to_value(&failure.error).unwrap_or_else(|_| json!({ "message": failure.error.message }));
            if let Some(error) = error.as_object_mut() {
                error.insert("operation".into(), json!(failure.operation));
            }
            self.json_line("error", json!({ "exit": failure.exit.code(), "error": error }));
        } else {
            let _ = writeln!(self.stderr, "error: {}", failure.error.message);
            if let Some(op) = &failure.operation {
                let _ = writeln!(self.stderr, "operation: {op}");
            }
        }
        failure.exit
    }
}

/// `secret` as the details card shows it until asked: masked, unless `show`.
pub fn masked(secret: &str, show: bool) -> String {
    if show || secret.is_empty() { secret.to_owned() } else { "********".to_owned() }
}

#[cfg(test)]
mod tests {
    use super::{Exit, Failure, Output, masked};
    use crate::error::{Code, Error};

    #[test]
    fn a_failure_exits_by_whether_something_must_be_put_right_first() {
        assert_eq!(Exit::of(&Code::AlreadyInProgress), Exit::NotReady);
        assert_eq!(Exit::of(&Code::DockerNotRunning), Exit::NotReady);
        assert_eq!(Exit::of(&Code::Other), Exit::Failed);
        assert_eq!(Failure::from(Error::Invalid("lab `x` is not installed".into())).exit, Exit::NotReady);
        assert_eq!(Failure::from(Error::Invalid("invalid lab id `../x`".into())).exit, Exit::Usage);
        assert_eq!(Failure::from(Error::Invalid("the deploy failed".into())).exit, Exit::Failed);
        assert_eq!([Exit::Ok, Exit::Failed, Exit::Usage, Exit::NotReady].map(Exit::code), [0, 1, 2, 3]);
    }

    #[test]
    fn json_lines_carry_the_shape_version_the_type_and_the_command() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut o = Output::new(true, &mut out, &mut err);
        o.command = "labs.list".into();
        o.say("not in JSON");
        o.event(&serde_json::json!({ "kind": "log", "line": "x" }), Some("x"));
        assert_eq!(o.finish(Ok(serde_json::json!([])), |_| String::new()), Exit::Ok);
        let failure = Failure { operation: Some("op-1".into()), ..Failure::not_ready("busy") };
        assert_eq!(o.fail(&failure), Exit::NotReady);
        let lines: Vec<serde_json::Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0], serde_json::json!({ "v": 1, "type": "event", "command": "labs.list", "event": { "kind": "log", "line": "x" } }));
        assert_eq!(lines[1], serde_json::json!({ "v": 1, "type": "result", "command": "labs.list", "data": [] }));
        assert_eq!(
            lines[2],
            serde_json::json!({ "v": 1, "type": "error", "command": "labs.list", "exit": 3, "error": { "code": "refused", "message": "busy", "operation": "op-1" } })
        );
        assert_eq!(String::from_utf8_lossy(&err), "", "nothing on stderr in JSON");
    }

    #[test]
    fn a_secret_is_masked_unless_asked() {
        assert_eq!(masked("hunter2", false), "********");
        assert_eq!(masked("hunter2", true), "hunter2");
        assert_eq!(masked("", false), "");
    }
}
