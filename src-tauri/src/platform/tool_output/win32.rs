//! The error number a Windows tool's message ends with, read: `shutdown.exe`, like most of
//! Windows' own tools, ends its message with the Win32 error in parentheses (`A system shutdown
//! has already been scheduled.(1190)`) in every display language, so the number, not the words,
//! says which error it was.

/// The Win32 error `text` ends with (its last `(<number>)`), if it ends with one.
#[must_use]
pub fn error_code(text: &str) -> Option<u32> {
    let text = text.trim_end().trim_end_matches('.');
    let inner = text.strip_suffix(')')?;
    let (_, number) = inner.rsplit_once('(')?;
    number.trim().parse().ok()
}

/// `shutdown.exe`'s refusal because a shutdown is already scheduled (`ERROR_SHUTDOWN_IS_SCHEDULED`).
pub const ERROR_SHUTDOWN_IS_SCHEDULED: u32 = 1190;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_number_is_read_whatever_the_language() {
        assert_eq!(error_code("A system shutdown has already been scheduled.(1190)\r\n"), Some(ERROR_SHUTDOWN_IS_SCHEDULED));
        assert_eq!(error_code("Un arrêt du système a déjà été planifié.(1190)"), Some(1190));
        assert_eq!(error_code("Access is denied.(5)"), Some(5));
        assert_eq!(error_code("listening on port 1190"), None, "a number that isn't the error's");
        assert_eq!(error_code(""), None);
    }
}
