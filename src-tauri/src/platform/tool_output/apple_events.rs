//! What `osascript` says when an Apple event fails, read by its error number (`UTM got an error:
//! Application isn't running. (-600)`): the number ends the message in every language.

/// An Apple event error, by its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleEvent {
    /// `-600`: the application isn't running (UTM says it of itself while it boots a VM).
    AppNotRunning,
    /// `-1712`: the event timed out (UTM busy).
    TimedOut,
    /// `-1743`: macOS refused this app Apple events to the application (the Automation
    /// permission), which no wait fixes.
    NotAuthorized,
    /// Any other number.
    Other(i32),
}

impl AppleEvent {
    /// The error `text` (what `osascript` printed) ends with: its last `(-<number>)`; none when it
    /// names none.
    #[must_use]
    pub fn of(text: &str) -> Option<AppleEvent> {
        let code = text.match_indices("(-").filter_map(|(at, _)| {
            let rest = &text[at + 1..];
            let end = rest.find(')')?;
            rest[..end].parse::<i32>().ok()
        });
        Some(match code.last()? {
            -600 => AppleEvent::AppNotRunning,
            -1712 => AppleEvent::TimedOut,
            -1743 => AppleEvent::NotAuthorized,
            other => AppleEvent::Other(other),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_by_their_number() {
        let of = |text: &str| AppleEvent::of(text);
        assert_eq!(of("execution error: UTM got an error: Application isn't running. (-600)"), Some(AppleEvent::AppNotRunning));
        assert_eq!(of("UTM got an error: AppleEvent timed out. (-1712)\n"), Some(AppleEvent::TimedOut));
        assert_eq!(of("execution error: Not authorized to send Apple events to UTM. (-1743)"), Some(AppleEvent::NotAuthorized));
        assert_eq!(of("erreur d’exécution : Non autorisé à envoyer des Apple Events à UTM. (-1743)"), Some(AppleEvent::NotAuthorized));
        assert_eq!(of("execution error: Can’t get virtual machine \"x\". (-1728)"), Some(AppleEvent::Other(-1728)));
        assert_eq!(of("exit status 1"), None);
    }
}
