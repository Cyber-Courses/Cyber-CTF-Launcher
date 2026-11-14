//! What `pkexec` prints when it, not the command it runs, failed (polkit's exit 126 or 127),
//! read from one line of its error output.

/// Why pkexec didn't run the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The password prompt was dismissed or the password refused (`Error executing command as
    /// another user: Not authorized`).
    Refused,
    /// No polkit agent could show a prompt (`…: No authentication agent found.`, or, with no
    /// terminal either, `Error creating textual authentication agent: …`).
    NoAgent,
}

impl Refusal {
    /// The refusal `line` says, if it is pkexec's own; none for the command's output.
    #[must_use]
    pub fn of_line(line: &str) -> Option<Refusal> {
        let line = line.trim_end();
        if line.starts_with("Error creating textual authentication agent") {
            return Some(Refusal::NoAgent);
        }
        line.strip_prefix("Error executing command as another user")
            .map(|why| if why.contains("No authentication agent") { Refusal::NoAgent } else { Refusal::Refused })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkexecs_own_lines_are_read_as_polkit_prints_them() {
        assert_eq!(Refusal::of_line("Error executing command as another user: Not authorized\n"), Some(Refusal::Refused));
        assert_eq!(Refusal::of_line("Error executing command as another user: No authentication agent found."), Some(Refusal::NoAgent));
        assert_eq!(
            Refusal::of_line(
                "Error creating textual authentication agent: Error opening current controlling terminal for the process (`/dev/tty'): No such device or address"
            ),
            Some(Refusal::NoAgent)
        );
        assert_eq!(Refusal::of_line("sh: 1: gpg: not found"), None, "the script's own output");
    }
}
