//! Secrets kept out of what the app writes down for people to read and send on: the log files,
//! the command lines a setup log shows and a lab's deploy log. A line is matched against the shapes
//! secrets take (an `Authorization` scheme, a password in a URL, a value under a secret-sounding
//! name, a JWT, a flag, cloud and Git hosting tokens, a private key) and each secret is replaced,
//! the rest of the line kept. A secret of no such shape that the app holds (an attack box's
//! password, a server's, a launch's flags) is [`register`]ed: it is replaced wherever it shows. A
//! log written a line at a time goes through a [`Redactor`], which also drops the lines of a
//! private key between its `BEGIN` and `END` lines.

use std::borrow::Cow;
use std::sync::{LazyLock, PoisonError, RwLock};

use regex::Regex;

/// What a secret is replaced with.
const REDACTED: &str = "[redacted]";

/// Each shape a secret takes, and its replacement (`$name` keeps that part of the match, `{R}` is
/// [`REDACTED`]).
const PATTERNS: [(&str, &str); 11] = [
    // A private key, whole (a text of several lines, or one with escaped line breaks); a key cut
    // short is replaced to the end.
    (r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----(?s:.*?)(?:-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)", "{R}"),
    // `Authorization: Bearer <token>`, `Basic <base64>`.
    (r"(?i)\b(?P<scheme>bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}", "$scheme {R}"),
    // A password in a URL: `https://user:password@host`.
    (r"(?i)(?P<user>\b[a-z][a-z0-9+.-]*://[^/\s:@]+):[^@\s/]+@", "$user:{R}@"),
    // A value under a secret-sounding name: `token=…`, `password: …`, `"client_secret":"…"`.
    (
        r#"(?i)(?P<key>\b[a-z0-9_.-]*(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key|credential|authorization|ctf_flag)[a-z0-9_.-]*"?\s*[:=]\s*"?)(?:bearer\s+|basic\s+)?[^\s",&;}]+"#,
        "${key}{R}",
    ),
    // A JSON Web Token: three base64url parts, the first a JSON header.
    (r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}", "{R}"),
    // An AWS access key id.
    (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", "{R}"),
    // A flag in the usual CTF shapes: `flag{…}`, `CTF{…}`, `XYZCTF{…}`.
    (r"(?i)\b(?:flag|[a-z0-9_]*ctf)\{[^}\s]{1,256}\}", "{R}"),
    // GitHub tokens (classic, OAuth, app, refresh) and fine-grained ones; GitLab's.
    (r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255}|glpat-[A-Za-z0-9_-]{20,255})\b", "{R}"),
    // DigitalOcean tokens, Google API keys.
    (r"\b(?:do[por]_v1_[a-f0-9]{64}|AIza[0-9A-Za-z_-]{35})\b", "{R}"),
    // An Azure client secret (`xxx8Q~…`).
    (r"\b[A-Za-z0-9_.-]{3}[0-9]Q~[A-Za-z0-9_~.-]{31,34}", "{R}"),
    // An AWS secret access key on its own: 40 base64 characters, checked further by
    // `bare_aws_secrets` (a Git commit or a digest is hex, never mixed case).
    (r"[A-Za-z0-9/+]{40}", "{AWS}"),
];

/// [`PATTERNS`], compiled (a test checks every one compiles).
static RULES: LazyLock<Vec<(Regex, Replace)>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .filter_map(|(pattern, replacement)| {
            let replace = if *replacement == "{AWS}" { Replace::BareAwsSecret } else { Replace::With(replacement.replace("{R}", REDACTED)) };
            Some((Regex::new(pattern).ok()?, replace))
        })
        .collect()
});

/// How a pattern's match is replaced.
enum Replace {
    /// By this text (`$name` keeps that part of the match).
    With(String),
    /// By [`REDACTED`] when it is a whole word of mixed case and digits (`bare_aws_secret`).
    BareAwsSecret,
}

/// Whether `m`, a run of 40 base64 characters in `text`, is an AWS secret access key: not part of
/// a longer run, and with upper case, lower case and digits (a hex digest has no mixed case).
fn bare_aws_secret(text: &str, m: &regex::Match<'_>) -> bool {
    let base64 = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '+' | '=');
    let alone = !text[..m.start()].chars().next_back().is_some_and(base64) && !text[m.end()..].chars().next().is_some_and(base64);
    let s = m.as_str();
    alone && s.chars().any(|c| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase()) && s.chars().any(|c| c.is_ascii_digit())
}

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
    if let Cow::Owned(replaced) = by_pattern(&out) {
        return Cow::Owned(replaced);
    }
    out
}

/// `text` with what the patterns take replaced by `[redacted]` (the secrets [`register`] was
/// given aside); borrowed when they took nothing.
fn by_pattern(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for (pattern, replace) in RULES.iter() {
        let replaced = match replace {
            Replace::With(replacement) => pattern.replace_all(&out, replacement.as_str()),
            Replace::BareAwsSecret => {
                let text: &str = &out;
                pattern.replace_all(text, |c: &regex::Captures<'_>| {
                    c.get(0).map_or_else(String::new, |m| if bare_aws_secret(text, &m) { REDACTED.to_owned() } else { m.as_str().to_owned() })
                })
            }
        };
        // A match the rule kept as it was (a run of 40 characters that is no AWS secret) changes
        // nothing: the text stays borrowed.
        if let Cow::Owned(replaced) = replaced
            && replaced != *out
        {
            out = Cow::Owned(replaced);
        }
    }
    out
}

/// Redacts a log written a line at a time: each line as [`redact`] does, and every line of a
/// private key from its `BEGIN` line to its `END` line, which no single line shows whole.
#[derive(Debug, Default)]
pub struct Redactor {
    /// A private key's `BEGIN` line was seen, and not its `END` yet.
    in_private_key: bool,
}

impl Redactor {
    /// `line`, redacted, as the log should keep it.
    pub fn line(&mut self, line: &str) -> String {
        let begins = PRIVATE_KEY_BEGIN.as_ref().and_then(|re| re.find(line));
        let ends = PRIVATE_KEY_END.as_ref().and_then(|re| re.find(line));
        if self.in_private_key {
            if let Some(end) = ends {
                self.in_private_key = false;
                return format!("{REDACTED}{}", redact(&line[end.end()..]));
            }
            return REDACTED.to_owned();
        }
        match (begins, ends) {
            (Some(begin), None) => {
                self.in_private_key = true;
                format!("{}{REDACTED}", redact(&line[..begin.start()]))
            }
            _ => redact(line).into_owned(),
        }
    }
}

/// `text`, a log of several lines, redacted as a [`Redactor`] writing it would have (its line
/// breaks kept).
pub fn redact_lines(text: &str) -> String {
    let mut r = Redactor::default();
    text.split_inclusive('\n')
        .map(|l| {
            let (body, end) = l.strip_suffix('\n').map_or((l, ""), |b| (b, "\n"));
            r.line(body) + end
        })
        .collect()
}

/// A private key's first line, and its last (None never happens: a test compiles them).
static PRIVATE_KEY_BEGIN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----").ok());
static PRIVATE_KEY_END: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----").ok());

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
    fn flags_tokens_and_keys_are_replaced() {
        let aws_secret = samples::aws_secret();
        let [begin, body, _, end] = samples::private_key_lines();
        let cases = [
            ("CTF_FLAG_MAIN=s3cr3t-value-1 vagrant up".to_owned(), "CTF_FLAG_MAIN=[redacted] vagrant up".to_owned()),
            ("echo flag{th1s_is_the_fl4g} > /srv/flag".to_owned(), "echo [redacted] > /srv/flag".to_owned()),
            ("found CyberCTF{abc-123} in the dump".to_owned(), "found [redacted] in the dump".to_owned()),
            (format!("git clone https://x@github.com {}", samples::github_classic()), "git clone https://x@github.com [redacted]".to_owned()),
            (format!("token {}", samples::github_fine_grained()), "token [redacted]".to_owned()),
            (format!("DO {}", samples::digitalocean()), "DO [redacted]".to_owned()),
            (format!("gcp {} end", samples::google_api_key()), "gcp [redacted] end".to_owned()),
            (format!("azure {} ok", samples::azure_secret()), "azure [redacted] ok".to_owned()),
            (format!("aws {aws_secret} ok"), "aws [redacted] ok".to_owned()),
            (format!(r#"{{"key":"{begin}\n{body}\n{end}\n","n":1}}"#), r#"{"key":"[redacted]\n","n":1}"#.to_owned()),
            (format!("{begin}\n{body}\n{end}\nafter"), "[redacted]\nafter".to_owned()),
        ];
        for (line, expected) in cases {
            assert_eq!(by_pattern(&line), expected, "{line}");
        }
        // A Git commit, a digest, a long base64 run: not an AWS secret.
        for kept in ["commit 0a2715b0a2715b0a2715b0a2715b0a2715b0a271", "sha256:ABCDEF0123456789ABCDEF0123456789ABCDEF01", &format!("{aws_secret}{aws_secret}")]
        {
            assert!(matches!(by_pattern(kept), Cow::Borrowed(_)), "{kept}");
        }
    }

    #[test]
    fn a_private_key_written_a_line_at_a_time_is_dropped_from_begin_to_end() {
        let mut r = Redactor::default();
        let [begin, body, more, end] = samples::private_key_lines();
        let lines = [format!("writing the key: {begin}"), body, more, format!("{end} done"), "next line".to_owned()];
        let out: Vec<String> = lines.iter().map(|l| r.line(l)).collect();
        assert_eq!(out, ["writing the key: [redacted]", "[redacted]", "[redacted]", "[redacted] done", "next line"]);
        assert_eq!(Redactor::default().line("token=abc123"), "token=[redacted]", "the patterns too");
        assert_eq!(redact_lines(&(lines.join("\n") + "\n")), "writing the key: [redacted]\n[redacted]\n[redacted]\n[redacted] done\nnext line\n");
    }

    #[test]
    fn every_pattern_compiles() {
        assert_eq!(RULES.len(), PATTERNS.len());
        assert!(PRIVATE_KEY_BEGIN.is_some() && PRIVATE_KEY_END.is_some());
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
            assert!(matches!(by_pattern(line), Cow::Borrowed(_)), "{line}");
        }
    }
}

/// Secrets of each shape the patterns know, for tests here and of the logs that use them. Built
/// from pieces, so a secret scanner reading the sources doesn't take them for leaked ones.
#[cfg(test)]
pub(crate) mod samples {
    /// "PRIVATE KEY", in two pieces.
    const PRIVATE_KEY: &str = concat!("PRIVATE", " KEY");

    /// A private key's lines: its `BEGIN` line, two of its body, its `END` line.
    pub(crate) fn private_key_lines() -> [String; 4] {
        [
            format!("-----BEGIN OPENSSH {PRIVATE_KEY}-----"),
            "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW".to_owned(),
            "QyNTUxOQAAACDxkeyMaterial".to_owned(),
            format!("-----END OPENSSH {PRIVATE_KEY}-----"),
        ]
    }

    /// A classic GitHub token.
    pub(crate) fn github_classic() -> String {
        ["gh", "p_", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"].concat()
    }

    /// A fine-grained GitHub token.
    pub(crate) fn github_fine_grained() -> String {
        ["github", "_pat_", "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz"].concat()
    }

    /// A DigitalOcean token.
    pub(crate) fn digitalocean() -> String {
        ["do", "p_v1_", &"0123456789abcdef".repeat(4)].concat()
    }

    /// A Google API key.
    pub(crate) fn google_api_key() -> String {
        ["AI", "zaSyA-1234567890abcdefghijklmnopqrstu"].concat()
    }

    /// An Azure client secret.
    pub(crate) fn azure_secret() -> String {
        ["abc8", "Q~", "0123456789abcdefghijklmnopqrstuvwx"].concat()
    }

    /// An AWS secret access key (AWS's documentation example).
    pub(crate) fn aws_secret() -> String {
        ["wJalrXUtnFEMI/K7MDENG", "/bPxRfiCYEXAMPLEKEY"].concat()
    }
}
