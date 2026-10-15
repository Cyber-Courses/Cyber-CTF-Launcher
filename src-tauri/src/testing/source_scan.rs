//! Rules over the crate's own source that no single unit test can see: every Tauri command is
//! called from a test (lib.rs), every literal argument a cmd.exe-wrapped tool gets passes its
//! guard, and nothing listens beyond the loopback.

use std::path::{Path, PathBuf};

/// A Rust source file of the crate: its path, the code that ships, and its test code.
pub struct Source {
    pub path: PathBuf,
    pub code: String,
    pub tests: String,
}

/// Every `.rs` file under `src/`, split into shipped and test code.
pub fn crate_sources() -> Vec<Source> {
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<Source>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            let src = std::fs::read_to_string(&path).unwrap();
            let (code, tests) = split_tests(&path, &src);
            out.push(Source { path, code, tests });
        }
    }
}

/// Whole test files (`tests.rs`, `proptests.rs`) are test code, and the test-only helpers neither;
/// any other file is cut at its `#[cfg(test)]` module: what comes before ships, the rest is tests.
fn split_tests(path: &Path, src: &str) -> (String, String) {
    const TEST_FILES: &[&str] = &["tests.rs", "proptests.rs"];
    const TEST_HELPERS: &[&str] = &["fake.rs", "test_app.rs", "proptest_support.rs", "source_scan.rs"];
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if TEST_FILES.contains(&name) {
        return (String::new(), src.to_string());
    }
    if TEST_HELPERS.contains(&name) {
        return (String::new(), String::new());
    }
    let lines: Vec<&str> = src.lines().collect();
    let start = (0..lines.len()).find(|&i| {
        let attr = lines[i].trim();
        let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or_default();
        (attr == "#[cfg(test)]" || attr.starts_with("#[cfg(all(test"))
            && (next.starts_with("mod ") || next.starts_with("pub(crate) mod "))
            && next.ends_with('{')
    });
    match start {
        Some(i) => (lines[..i].join("\n"), lines[i..].join("\n")),
        None => (src.to_string(), String::new()),
    }
}

/// The string literals among the elements of the bracketed list `code` starts with (`[` or
/// `vec![`), unescaped; elements that aren't literals (a variable, a `format!`) are skipped.
pub fn literal_list(code: &str) -> Vec<String> {
    let mut chars = code.char_indices().skip_while(|(_, c)| *c != '[').peekable();
    let mut literals = Vec::new();
    let mut depth = 0;
    while let Some((at, c)) = chars.next() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            '"' => {
                let hashes = code[..at].len() - code[..at].trim_end_matches('#').len();
                let raw = code[..at - hashes].ends_with('r');
                let close = format!("\"{}", "#".repeat(hashes));
                let mut s = String::new();
                while let Some((i, c)) = chars.next() {
                    match c {
                        '"' if code[i..].starts_with(&close) => {
                            for _ in 0..hashes {
                                chars.next();
                            }
                            break;
                        }
                        '\\' if !raw => match chars.next().map(|(_, e)| e) {
                            Some('n') => s.push('\n'),
                            Some('r') => s.push('\r'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => {}
                        },
                        _ => s.push(c),
                    }
                }
                if depth == 1 {
                    literals.push(s);
                }
            }
            _ => {}
        }
    }
    literals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_lists_are_read_with_their_escapes_and_nested_calls_skipped() {
        assert_eq!(literal_list(r#"&["version", "--query", "\"azure-cli\""], None)"#), ["version", "--query", "\"azure-cli\""]);
        assert_eq!(literal_list(r##"vec!["a", &format!("{x}", "no"), b, r"c\d", r#"q"q"#]"##), ["a", r"c\d", r#"q"q"#]);
        assert_eq!(literal_list(r#"["x]y", "z"]; ["after"]"#), ["x]y", "z"]);
        assert!(literal_list("no list here").is_empty());
    }

    #[test]
    fn a_files_test_module_is_test_code() {
        let src = "fn shipped() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let (code, tests) = split_tests(Path::new("x.rs"), src);
        assert_eq!(code, "fn shipped() {}");
        assert!(tests.starts_with("#[cfg(test)]") && tests.contains("fn t()"));
        assert_eq!(split_tests(Path::new("a/proptests.rs"), "fn p() {}").0, "");
        // A test-only item that isn't a module stays with the code (it is cfg'd out anyway).
        assert_eq!(split_tests(Path::new("x.rs"), "#[cfg(test)]\nmod proptests;\n").1, "");
    }

    #[test]
    fn the_crate_is_read_whole() {
        let sources = crate_sources();
        assert!(sources.len() > 100, "{}", sources.len());
        assert!(sources.iter().any(|s| s.path.ends_with("exec/shim.rs") && s.code.contains("fn check_args") && s.tests.contains("mod tests")));
    }
}
