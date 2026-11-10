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
            let (code, tests) = if declared_for_tests(&path) { (String::new(), src) } else { split_tests(&path, &src) };
            out.push(Source { path, code, tests });
        }
    }
}

/// Whether the file at `path` is a module its parent declares for tests only (`#[cfg(test)]`
/// over its `mod` line, as `runtime::fake_clis` is): all of it is test code, whatever its name.
fn declared_for_tests(path: &Path) -> bool {
    let (Some(dir), Some(name)) = (path.parent(), path.file_stem().and_then(|n| n.to_str())) else { return false };
    let Ok(parent) = std::fs::read_to_string(dir.with_extension("rs")) else { return false };
    let lines: Vec<&str> = parent.lines().map(str::trim).collect();
    let declares = |l: &str| l.strip_suffix(';').and_then(|l| l.rsplit(' ').next()) == Some(name) && l.contains("mod ");
    lines.iter().enumerate().filter(|(_, l)| declares(l)).any(|(i, _)| {
        // The attributes right above the `mod` line.
        lines[..i].iter().rev().take_while(|l| l.starts_with("#[")).any(|a| *a == "#[cfg(test)]" || a.starts_with("#[cfg(all(test"))
    })
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
        assert_eq!(literal_list("no list here"), [] as [&str; 0]);
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
    fn a_module_declared_for_tests_is_test_code_whatever_its_name() {
        let fake_clis = crate_sources().into_iter().find(|s| s.path.ends_with("runtime/fake_clis.rs")).expect("runtime/fake_clis.rs");
        assert!(fake_clis.code.is_empty() && fake_clis.tests.contains("struct FakeBin"));
        let lifecycle = crate_sources().into_iter().find(|s| s.path.ends_with("runtime/lifecycle.rs")).expect("runtime/lifecycle.rs");
        assert!(lifecycle.code.contains("fn stop_lab"));
    }

    #[test]
    fn the_crate_is_read_whole() {
        let sources = crate_sources();
        assert!(sources.len() > 100, "{}", sources.len());
        assert!(sources.iter().any(|s| s.path.ends_with("exec/shim.rs") && s.code.contains("fn check_args") && s.tests.contains("mod tests")));
    }
}

/// The layers depend downwards only: the platform (running tools, files, the OS) names neither the
/// labs' runtime nor the app shell; the runtime doesn't name the app shell, nor, outside its
/// tests, the layers that use it (the CTF's labs, the account, the machine, the commands): what
/// it needs of them is handed in (a source check, a hook to run first). A cycle between them
/// came back more than once (the runtime reached for the deploy workers' lock and status files,
/// then for the catalogue's source rule, the co-location server and Storage's records).
#[test]
fn the_platform_and_the_runtime_never_reach_up_into_the_layers_above() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // Spelled in two, so this file doesn't name the layers itself.
    let path = |layer: &str| ["crate::", layer].concat();
    let (runtime, app) = (path("runtime"), path("app::"));
    let above_runtime = ["labs", "account", "machine", "commands"].map(|l| (path(l), [path(l), "::".into()].concat(), [path(l), ";".into()].concat()));
    for s in crate_sources() {
        let rel = s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        let text = format!("{}\n{}", s.code, s.tests);
        let layer = rel.split(['/', '.']).next().unwrap_or_default();
        if layer == "platform" {
            assert!(!text.contains(&runtime) && !text.contains(&app), "{rel} reaches up from the platform");
        }
        if layer == "runtime" {
            assert!(!text.contains(&app), "{rel} reaches up from the runtime into the app shell");
            for (name, item, whole) in &above_runtime {
                assert!(!s.code.contains(item.as_str()) && !s.code.contains(whole.as_str()), "{rel} reaches up from the runtime into {name}");
            }
            // And through a grouped `use crate::{…}`.
            let grouped = s.code.lines().map(str::trim).filter(|l| l.starts_with(&["use ", &path("{")].concat()));
            for line in grouped {
                let names: Vec<&str> = line.split(['{', '}', ',', ' ', ':', ';']).collect();
                assert!(!["labs", "account", "machine", "commands"].iter().any(|l| names.contains(l)), "{rel} reaches up from the runtime: {line}");
            }
        }
    }
}
