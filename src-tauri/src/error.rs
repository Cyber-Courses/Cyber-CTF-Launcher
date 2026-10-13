use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    /// An operation refused before it changed anything (a lab still up elsewhere, an update of a
    /// running lab): not a failed deploy, so the lab's last verdict stays what it was.
    #[error("{0}")]
    Refused(String),
    #[error("`{tool}` is not installed or not on PATH")]
    ToolMissing { tool: &'static str },
    #[error("`{command}` failed: {stderr}")]
    CommandFailed { command: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// Commands return errors to the webview as plain strings.
impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_reach_the_webview_as_their_message() {
        let cases = [
            (Error::Invalid("bad id".into()), "bad id"),
            (Error::Refused("still running".into()), "still running"),
            (Error::ToolMissing { tool: "vagrant" }, "`vagrant` is not installed or not on PATH"),
            (Error::CommandFailed { command: "docker ps".into(), stderr: "daemon down".into() }, "`docker ps` failed: daemon down"),
            (Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "no such file")), "no such file"),
        ];
        for (error, message) in cases {
            assert_eq!(serde_json::to_value(&error).unwrap(), serde_json::json!(message));
        }
    }
}
