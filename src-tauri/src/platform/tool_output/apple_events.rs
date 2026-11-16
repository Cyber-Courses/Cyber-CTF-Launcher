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

/// Whether `text` (what Vagrant's UTM plugin printed) is the system refusing UTM the action: an
/// Apple event UTM answered with the system's "Operation not permitted" (`utmctl start`'s `Error
/// from event: … (OSStatus error -2700.) … Operation not permitted`), or one macOS didn't let this
/// app send (`-1743`). The runner reads its own Vagrant calls the same way (`provider_refused`,
/// need `permission`); the VM self-test runs Vagrant itself.
#[must_use]
pub fn refused_by_the_system(text: &str) -> bool {
    let lower = text.to_lowercase();
    let event_failed = lower.contains("osstatus error") || lower.contains("error from event") || AppleEvent::of(text).is_some();
    (event_failed && lower.contains("operation not permitted")) || AppleEvent::of(text) == Some(AppleEvent::NotAuthorized)
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

    #[test]
    fn the_system_refusing_utm_is_read_from_what_vagrant_utm_printed() {
        let start = "There was an error while executing `utmctl`, a CLI used by vagrant-utm\nfor controlling UTM. The command and stderr is shown below.\n\nCommand: [\"start\", \"2D2607AB-D82F-4A13-A1FB-F025C1D06D12\"]\n\nStderr: Error from event: The operation couldn’t be completed. (OSStatus error -2700.)\nThe operation couldn’t be completed. Operation not permitted";
        assert!(refused_by_the_system(start));
        assert!(refused_by_the_system("execution error: Not authorized to send Apple events to UTM. (-1743)"));
        assert!(!refused_by_the_system("cp: /data/disk.img: Operation not permitted"), "not an Apple event");
        assert!(!refused_by_the_system("Error from event: The operation couldn’t be completed. (OSStatus error -2700.)"));
    }
}
