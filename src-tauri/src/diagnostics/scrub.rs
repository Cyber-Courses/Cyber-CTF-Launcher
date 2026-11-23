//! What keeps a diagnostics bundle safe to send: every text that goes into it passes through
//! [`Scrubber::scrub`]. It replaces, in order, the secrets the launcher knows by value (the launch
//! values this process holds: a run's token, its flags), every secret shape the log files' redaction knows
//! (`platform::redact`: tokens, passwords, keys, the secrets this process registered), e-mail
//! addresses, CTF flags (`flag{…}`, `CTF{…}`, `<event>ctf{…}`), and finally the user's home
//! folder, written as `~`.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::redact::redact_lines;

/// What a known secret, an e-mail address and a flag are replaced with.
const REDACTED: &str = "[redacted]";

/// The shortest known secret replaced by value: a shorter one would replace ordinary words (the
/// same floor as `redact::register`).
const SHORTEST: usize = 8;

/// An e-mail address.
static EMAIL: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}").ok());

/// A CTF flag: a word ending in `flag` or `ctf` (any case) and a braced value without spaces.
static FLAG: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?i)\b[a-z0-9_-]{0,24}(?:flag|ctf)\{[^{}\s]{1,256}\}").ok());

/// Replaces what must not leave this computer in the texts of a bundle.
pub(crate) struct Scrubber {
    /// The secrets known by value, longest first (a secret containing another goes whole).
    known: Vec<String>,
    /// The home folder as it may be written (as is, JSON-escaped, with forward slashes), when
    /// one is known and long enough to be told from other text.
    home: Option<Regex>,
}

impl Scrubber {
    /// A scrubber for the user whose home folder is `home`, replacing the values `known` too.
    pub(crate) fn new(home: Option<&Path>, known: impl IntoIterator<Item = String>) -> Scrubber {
        let mut known: Vec<String> = known.into_iter().filter(|s| s.chars().count() >= SHORTEST).collect();
        known.sort_by_key(|s| std::cmp::Reverse(s.len()));
        known.dedup();
        Scrubber { known, home: home.and_then(home_pattern) }
    }

    /// `text` with every secret, address, flag and home folder in it replaced.
    pub(crate) fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for secret in &self.known {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        // As the logs are redacted, line after line: a private key's lines go from its BEGIN to its END.
        out = redact_lines(&out);
        for pattern in [&*EMAIL, &*FLAG].into_iter().flatten() {
            out = pattern.replace_all(&out, REDACTED).into_owned();
        }
        if let Some(home) = &self.home {
            out = home.replace_all(&out, "~").into_owned();
        }
        out
    }
}

/// The pattern of home folder `home` in a text: as written, JSON-escaped (a Windows path's
/// backslashes doubled) and with forward slashes, ending where a path segment can end (so
/// `/home/ada` doesn't take `/home/adam`'s start); case-insensitive on Windows, as its paths are.
/// None for a home too short to tell from other text (`/`, `C:\`).
fn home_pattern(home: &Path) -> Option<Regex> {
    let plain = home.to_string_lossy().trim_end_matches(['/', '\\']).to_owned();
    if plain.len() < 4 {
        return None;
    }
    let mut forms = vec![plain.replace('\\', "\\\\"), plain.replace('\\', "/"), plain];
    forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
    forms.dedup();
    let alternatives: Vec<String> = forms.iter().map(|f| regex::escape(f)).collect();
    let case = if cfg!(windows) { "(?i)" } else { "" };
    Regex::new(&format!(r"{case}(?:{})(?:\b|$)", alternatives.join("|"))).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber::new(Some(Path::new("/home/ada")), ["k7mpq-zrt4w-hn8xc".to_owned(), "short".to_owned()])
    }

    #[test]
    fn known_values_tokens_addresses_and_flags_are_replaced() {
        let s = scrubber();
        let cases = [
            ("launch k7mpq-zrt4w-hn8xc ok", "launch [redacted] ok"),
            ("Authorization: Bearer abcdefghijklmnop12345", "Authorization: [redacted]"),
            ("signed in as ada.lovelace@example.org", "signed in as [redacted]"),
            ("found FLAG{s3cr3t_v4lue} in /root", "found [redacted] in /root"),
            ("found cyberctf{abc-123} and htbctf{x}", "found [redacted] and [redacted]"),
            (r#"{"password":"hunter2hunter2"}"#, r#"{"password":"[redacted]"}"#),
        ];
        for (text, expected) in cases {
            assert_eq!(s.scrub(text), expected, "{text}");
        }
    }

    #[test]
    fn the_home_folder_reads_as_a_tilde_in_every_form_and_only_as_a_whole_segment() {
        let s = scrubber();
        assert_eq!(s.scrub("log at /home/ada/.local/share/x.log"), "log at ~/.local/share/x.log");
        assert_eq!(s.scrub("home is /home/ada"), "home is ~");
        assert_eq!(s.scrub("/home/adam/x"), "/home/adam/x", "another user's home stays");
        let windows = Scrubber::new(Some(Path::new(r"C:\Users\ada")), []);
        assert_eq!(windows.scrub(r#"{"path":"C:\\Users\\ada\\AppData"}"#), r#"{"path":"~\\AppData"}"#);
        assert_eq!(windows.scrub(r"C:\Users\ada\AppData and C:/Users/ada/x"), r"~\AppData and ~/x");
        assert!(Scrubber::new(Some(Path::new("/")), []).home.is_none(), "a root home would replace every path");
    }

    #[test]
    fn ordinary_text_is_kept() {
        let s = scrubber();
        for text in ["Cyber CTF 1.9.0 app starting: linux x86_64", "a short line", "Foo { a: 1 }", r#"{"labs":{"web":1}}"#, "root@pam on node pve"] {
            assert_eq!(s.scrub(text), text);
        }
    }
}
