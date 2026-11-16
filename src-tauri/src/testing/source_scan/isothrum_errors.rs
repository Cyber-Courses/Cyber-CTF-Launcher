//! Isothrum's errors become the launcher's in one place, `runtime/isothrum/errors.rs`
//! (docs/contributing/boundaries.md). Shipped code elsewhere that matches them (an
//! `isothrum_runner::Error` variant, under its own name or an alias, an `ErrorCode`, an
//! `ErrorDetail`) is an offender, unless [`ALLOWED`] lists it.
//!
//! [`ALLOWED`] is today's offenders, each with the step of the migration that removes it. It only
//! shrinks: a file not on it fails the build, and so does an entry whose file no longer offends.

/// The one file that converts Isothrum's errors.
const CONVERTER: &str = "runtime/isothrum/errors.rs";

/// The files allowed to match Isothrum's errors until the migration step named removes them.
const ALLOWED: &[(&str, &str)] = &[
    (
        "runtime/isothrum.rs",
        "L-7: the in-process runner's lock wait, the helper VM's refusal and the resume of nothing read a code; the one Deployment handle over the client takes them",
    ),
    ("runtime/isothrum/remote.rs", "L-7: the cancelled operation's report and the other client errors, with remote::in_process gone"),
];

/// Each line of `code` that matches one of Isothrum's errors, 1-based; `use` lines and comments
/// aside.
fn matches(code: &str) -> Vec<usize> {
    let base = regex::Regex::new(r"\bErrorDetail\b|\bErrorCode::|\bisothrum_runner::Error::").unwrap();
    let alias = regex::Regex::new(r"\bError as (\w+)").unwrap();
    let aliases: Vec<regex::Regex> = code
        .lines()
        .filter(|l| l.trim_start().starts_with("use ") && l.contains("isothrum_runner"))
        .flat_map(|l| alias.captures_iter(l).map(|c| c[1].to_owned()).collect::<Vec<_>>())
        .map(|a| regex::Regex::new(&format!(r"\b{a}::")).unwrap())
        .collect();
    code.lines()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.trim_start();
            !l.starts_with("use ") && !l.starts_with("//") && (base.is_match(l) || aliases.iter().any(|a| a.is_match(l)))
        })
        .map(|(n, _)| n + 1)
        .collect()
}

#[test]
fn the_error_rule_sees_a_variant_an_alias_a_code_and_a_detail_and_nothing_else() {
    assert_eq!(matches("let e = isothrum_runner::Error::NotRunning { run };"), [1]);
    assert_eq!(matches("use isothrum_runner::{Error as RunnerError, RunState};\nRunnerError::Locked { .. } => x,"), [2]);
    assert_eq!(matches("if e.code() == Some(isothrum_runner::ErrorCode::Locked) {}"), [1]);
    assert_eq!(matches("detail: Option<Box<ErrorDetail>>,"), [1]);
    assert_eq!(
        matches("use isothrum_runner::{ErrorCode, ErrorDetail};\n// ErrorCode::Locked\nfn f(e: isothrum_runner::Error) -> Error { errors::from_runner(e) }"),
        [] as [usize; 0]
    );
}

#[test]
fn isothrums_errors_are_converted_in_runtime_isothrum_errors_only() {
    let (found, stale) = super::beyond_allow_list(matches, &[CONVERTER], ALLOWED);
    assert_eq!(found, [] as [String; 0], "these read Isothrum's errors: convert them in {CONVERTER}");
    assert_eq!(stale, [] as [String; 0], "these no longer read Isothrum's errors: take them off ALLOWED");
}
