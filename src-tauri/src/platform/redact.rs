//! Secrets kept out of what the app writes down for people to read and send on: the log files
//! and the command lines a setup log shows. A line is matched against the shapes secrets take
//! (an `Authorization` scheme, a password in a URL, a value under a secret-sounding name, a JWT,
//! an AWS access key id) and each secret is replaced, the rest of the line kept. A secret of no
//! such shape that the app holds (an attack box's password) is [`register`]ed: it is replaced
//! wherever it shows.

use std::borrow::Cow;
use std::sync::{LazyLock, PoisonError, RwLock};

use regex::Regex;

/// What a secret is replaced with.
const REDACTED: &str = "[redacted]";

/// Each shape a secret takes, and its replacement (`$name` keeps that part of the match, `{R}` is
/// [`REDACTED`]).
const PATTERNS: [(&str, &str); 5] = [
    // `Authorization: Bearer <token>`, `Basic <base64>`.
    (r"(?i)\b(?P<scheme>bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}", "$scheme {R}"),
    // A password in a URL: `https://user:password@host`.
    (r"(?i)(?P<user>\b[a-z][a-z0-9+.-]*://[^/\s:@]+):[^@\s/]+@", "$user:{R}@"),
    // A value under a secret-sounding name: `token=…`, `password: …`, `"client_secret":"…"`.
    (
        r#"(?i)(?P<key>\b[a-z0-9_.-]*(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key|credential|authorization)[a-z0-9_.-]*"?\s*[:=]\s*"?)(?:bearer\s+|basic\s+)?[^\s",&;}]+"#,
        "${key}{R}",
    ),
    // A JSON Web Token: three base64url parts, the first a JSON header.
    (r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}", "{R}"),
    // An AWS access key id.
    (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "{R}"),
];

/// [`PATTERNS`], compiled (a test checks every one compiles).
static RULES: LazyLock<Vec<(Regex, String)>> =
    LazyLock::new(|| PATTERNS.iter().filter_map(|(pattern, replacement)| Some((Regex::new(pattern).ok()?, replacement.replace("{R}", REDACTED)))).collect());

/// The secrets the app holds, each replaced as it is ([`register`]).
static KNOWN: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// The shortest secret [`register`] takes: a shorter one would replace ordinary words.
const SHORTEST: usize = 8;

/// Redacts `secret` from every line from now on, in this process: a value the app holds (an
/// attack box's password) that no pattern would recognise. One shorter than 8 characters is
/// refused (it would replace ordinary words); registering it again changes nothing.
pub fn register(secret: &str) {
    if secret.chars().count() < SHORTEST {
        return;
    }
    let mut known = KNOWN.write().unwrap_or_else(PoisonError::into_inner);
    if !known.iter().any(|k| k == secret) {
        known.push(secret.to_owned());
    }
}

/// `line` with every secret in it replaced by `[redacted]`; borrowed when there was none.
pub fn redact(line: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(line);
    for secret in KNOWN.read().unwrap_or_else(PoisonError::into_inner).iter() {
        if out.contains(secret.as_str()) {
            out = Cow::Owned(out.replace(secret.as_str(), REDACTED));
        }
    }
    for (pattern, replacement) in RULES.iter() {
        if let Cow::Owned(replaced) = pattern.replace_all(&out, replacement.as_str()) {
            out = Cow::Owned(replaced);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_passwords_are_replaced_and_the_rest_kept() {
        let cases = [
            ("POST /graphql Authorization: Bearer abcdefghijklmnop12345", "POST /graphql Authorization: [redacted]"),
            ("header: Basic dXNlcjpwYXNzd29yZA==", "header: Basic [redacted]"),
            ("proxy http://ada:s3cr3t@proxy.corp:3128 set", "proxy http://ada:[redacted]@proxy.corp:3128 set"),
            ("callback?code=abc&access_token=xyz123&state=1", "callback?code=abc&access_token=[redacted]&state=1"),
            (r#"{"refresh_token":"r-123","name":"Ada"}"#, r#"{"refresh_token":"[redacted]","name":"Ada"}"#),
            ("PROXMOX_PASSWORD=hunter2 terraform apply", "PROXMOX_PASSWORD=[redacted] terraform apply"),
            ("client_secret: abc def", "client_secret: [redacted] def"),
            ("api-key = k-1", "api-key = [redacted]"),
            ("id token eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.c2lnbmF0dXJl here", "id token [redacted] here"),
            ("aws key AKIAIOSFODNN7EXAMPLE used", "aws key [redacted] used"),
        ];
        for (line, expected) in cases {
            assert_eq!(redact(line), expected, "{line}");
        }
    }

    #[test]
    fn a_registered_secret_is_replaced_wherever_it_shows() {
        let secret = "k7mpq-zrt4w-hn8xc-ls2vd";
        let line = format!("su ok, typed {secret} at the prompt, {secret}!");
        assert_eq!(redact(&line), line, "not known yet: no pattern takes it");
        register(secret);
        register(secret);
        assert_eq!(redact(&line), "su ok, typed [redacted] at the prompt, [redacted]!");
        assert_eq!(KNOWN.read().unwrap().iter().filter(|k| *k == secret).count(), 1, "kept once");
        register("short");
        assert!(matches!(redact("a short line"), Cow::Borrowed(_)), "too short to register: words stay");
    }

    #[test]
    fn every_pattern_compiles() {
        assert_eq!(RULES.len(), PATTERNS.len());
    }

    #[test]
    fn a_line_without_secrets_is_left_as_it_is() {
        for line in [
            "Cyber CTF 1.8.9 app starting: macos aarch64",
            "lab session s-2 failed: the lab host didn't answer",
            "the launch token was refused by the backend",
            "https://github.com/Cyber-Courses/Cyber-CTF-Launcher/releases/latest/download/latest.json",
            "$ pkexec env https_proxy=http://proxy.corp:3128 sh -c apt-get update",
        ] {
            assert!(matches!(redact(line), Cow::Borrowed(_)), "{line}");
        }
    }
}
