//! The crate's own source, read whole and split into shipped and test code, for the rules no
//! single unit test can see. Here: the layers depend downwards only, no file ships more than its
//! share of code nor is a `mod.rs`, `tauri` is used only in the shell and the commands, and
//! nothing listens beyond the loopback. Elsewhere, over the same split: every Tauri command is
//! called from a test and none that opens a window is synchronous (`commands.rs`), every literal
//! argument a cmd.exe-wrapped tool gets passes its guard (`platform/exec/shim.rs`), and only
//! `runtime/lab/` uses Isoloom's spec and generator crates, only `runtime/isothrum/` its run
//! manifest's (`runtime/lab.rs`).

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

/// Whether the file at `path` is test code whole: a module declared for tests only (`#[cfg(test)]`
/// over its `mod` line in its parent, as `runtime::fake_clis` is), or one inside such a module
/// (everything under `testing/` and `integration_tests/`), whatever its name.
fn declared_for_tests(path: &Path) -> bool {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut module = path.to_path_buf();
    while let (Some(dir), Some(name)) = (module.parent().map(Path::to_path_buf), module.file_stem().and_then(|n| n.to_str()).map(str::to_owned)) {
        let at_root = dir == src;
        let parent = if at_root { src.join("lib.rs") } else { dir.with_extension("rs") };
        if declares_for_tests(&parent, &name) {
            return true;
        }
        if at_root {
            return false;
        }
        module = parent;
    }
    false
}

/// Whether the file at `parent` declares its module `name` for tests only.
fn declares_for_tests(parent: &Path, name: &str) -> bool {
    let Ok(parent) = std::fs::read_to_string(parent) else { return false };
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
    fn a_module_inside_one_declared_for_tests_is_test_code() {
        let sources = crate_sources();
        let file = |end: &str| sources.iter().find(|s| s.path.ends_with(end)).unwrap_or_else(|| panic!("{end}"));
        for end in ["testing/logs.rs", "integration_tests/support.rs"] {
            assert!(file(end).code.is_empty() && !file(end).tests.is_empty(), "{end}");
        }
        assert_ne!(file("runtime/lifecycle.rs").code, "");
    }

    #[test]
    fn the_crate_is_read_whole() {
        let sources = crate_sources();
        assert!(sources.len() > 100, "{}", sources.len());
        assert!(sources.iter().any(|s| s.path.ends_with("exec/shim.rs") && s.code.contains("fn check_args") && s.tests.contains("mod tests")));
    }
}

/// The crate-root modules `code` names, outside whole-line comments: each `crate::<module>`, and
/// each item of a grouped `use crate::{…}`. A layer that reaches another through a module at the
/// crate root (the platform's process runner once called up into the runtime through one) names
/// it here as surely as one that names the layer itself.
fn crate_modules(code: &str) -> std::collections::BTreeSet<String> {
    let ident = |s: &str| s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>();
    let prefix = ["crate", "::"].concat();
    let mut out = std::collections::BTreeSet::new();
    for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
        for (at, _) in line.match_indices(&prefix) {
            let rest = &line[at + prefix.len()..];
            let Some(group) = rest.strip_prefix('{') else {
                out.insert(ident(rest));
                continue;
            };
            // The first name of each top-level item: `{a::b, c, d::{e, f}}` names a, c and d.
            let mut depth = 0;
            let mut item = true;
            for (i, c) in group.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' if depth == 0 => break,
                    '}' => depth -= 1,
                    ',' if depth == 0 => item = true,
                    c if item && (c.is_alphanumeric() || c == '_') => {
                        out.insert(ident(&group[i..]));
                        item = false;
                    }
                    _ => {}
                }
            }
        }
    }
    out.remove("");
    out
}

#[test]
fn a_files_crate_modules_are_read_from_paths_and_grouped_uses() {
    let code = [
        "use crate::platform::exec;",
        "use crate::{error::Result, runtime, labs::{a, b}};",
        "    let x = crate::machine::system::probe();",
        "/// Not code: see `crate::app`.",
        "    // Nor this: crate::commands.",
    ]
    .join("\n");
    assert_eq!(crate_modules(&code).into_iter().collect::<Vec<_>>(), ["error", "labs", "machine", "platform", "runtime"]);
}

/// The layers depend downwards only: the platform (running tools, files, the OS) names no module
/// at the crate root but its own and the crate's errors, so it can't reach up through one either;
/// the runtime names neither the app shell nor the layers that use it (the CTF's labs, the
/// account, the machine, the cloud sign-ins, the commands), its tests included (a test of a command over it belongs
/// with the command); and those domain layers name neither the app shell nor the commands over
/// them. What a layer needs from above is handed in (a source
/// check, a hook to run first, whether a deploy worker is busy). A cycle between them came back
/// more than once (the runtime reached for the deploy workers' lock and status files, then for
/// the catalogue's source rule, the co-location server and Storage's records; the platform's
/// process runner for the runtime's ESXi relay settings; the labs for the deploy workers' jobs;
/// the runtime's AWS budget for the cloud sign-ins' month).
#[test]
fn every_layer_names_only_the_layers_below_it() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let above_runtime = ["labs", "account", "machine", "cloud", "commands"];
    for s in crate_sources() {
        let rel = s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        let (code, tests) = (crate_modules(&s.code), crate_modules(&s.tests));
        let named = || code.iter().chain(&tests).map(String::as_str);
        match rel.split(['/', '.']).next().unwrap_or_default() {
            "platform" => {
                let up: Vec<&str> = named().filter(|m| !["platform", "error", "testing"].contains(m)).collect();
                assert!(up.is_empty(), "{rel} reaches up from the platform into {up:?}");
            }
            "runtime" => {
                let up: Vec<&str> = named().filter(|m| *m == "app" || above_runtime.contains(m)).collect();
                assert!(up.is_empty(), "{rel} reaches up from the runtime into {up:?}");
            }
            "labs" | "machine" | "account" | "cloud" => {
                let up: Vec<&str> = named().filter(|m| ["app", "commands"].contains(m)).collect();
                assert!(up.is_empty(), "{rel} reaches up from a domain layer into {up:?}");
            }
            _ => {}
        }
    }
}

/// Lines of code a file may ship before its tests (blank and comment lines not counted).
const MAX_FILE_LOC: usize = 400;

/// The lines of `code` that aren't blank or only a comment.
fn loc(code: &str) -> usize {
    code.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).count()
}

/// The crate's files, by their path under `src/`.
fn by_path() -> Vec<(String, Source)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    crate_sources().into_iter().map(|s| (s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"), s)).collect()
}

/// A file does one thing: none ships more than [`MAX_FILE_LOC`] lines of code (its tests aside),
/// and a module with children is `foo.rs` beside `foo/`, never `foo/mod.rs`. (These rules, and
/// where `tauri` may be used, read the split of each file into shipped and test code: they live
/// here, beside that split, and nowhere else.)
#[test]
fn no_file_ships_more_than_its_share_of_code_nor_is_a_mod_rs() {
    let over: Vec<String> =
        by_path().iter().map(|(rel, s)| (rel, loc(&s.code))).filter(|(_, n)| *n > MAX_FILE_LOC).map(|(rel, n)| format!("{rel}: {n}")).collect();
    assert_eq!(over, [] as [String; 0], "lines of code over {MAX_FILE_LOC}");
    let mod_rs: Vec<String> = by_path().into_iter().map(|(rel, _)| rel).filter(|rel| rel.ends_with("/mod.rs") || rel == "mod.rs").collect();
    assert_eq!(mod_rs, [] as [String; 0], "use foo.rs next to foo/");
}

/// Tauri is the app shell's and the commands' (and the crate root's): the domain and the layers
/// under it reach the player through what the shell hands them, so they run and are tested
/// without a webview.
#[test]
fn tauri_is_used_only_in_the_shell_and_the_commands() {
    let tauri = regex::Regex::new(r"\btauri(_plugin_\w+)?::|\b(AppHandle|WebviewWindow|AppRuntime)\b").unwrap();
    let adapters = regex::Regex::new(r"^((lib|main)\.rs|(app|commands)(\.rs|/))").unwrap();
    let outside: Vec<String> = by_path()
        .into_iter()
        .filter(|(rel, _)| !adapters.is_match(rel))
        .filter_map(|(rel, s)| {
            let line = s.code.lines().position(|l| !l.trim_start().starts_with("//") && tauri.is_match(l))?;
            Some(format!("{rel}:{}", line + 1))
        })
        .collect();
    assert_eq!(outside, [] as [String; 0], "domain code uses tauri: move it into commands/ or app/");
}

#[test]
fn the_source_rules_see_what_they_look_for() {
    assert_eq!(loc("fn a() {}\n\n  // a comment\n/// a doc\nfn b() {}\n"), 2);
    let tauri = regex::Regex::new(r"\btauri(_plugin_\w+)?::|\b(AppHandle|WebviewWindow|AppRuntime)\b").unwrap();
    assert!(tauri.is_match("use tauri::Manager;") && tauri.is_match("fn f(app: &AppHandle)") && tauri.is_match("tauri_plugin_dialog::init()"));
    assert!(!tauri.is_match("use ts_rs::TS;") && !tauri.is_match("let my_AppHandles = 1;"));
}

/// The user's home folder is read in one place, `platform::home_dir` (`USERPROFILE` on Windows,
/// `HOME` elsewhere). Regression: the OCI config, the dev session and the dev servers' secrets
/// read `HOME` first and the SSH key's Tauri's own lookup, so on Windows with `HOME` set (Git for
/// Windows, MSYS) the OCI setup looked for `%HOME%\.oci\config` where the OCI CLI reads
/// `%USERPROFILE%`.
#[test]
fn the_home_folder_is_read_only_through_platform_home_dir() {
    let home = regex::Regex::new(r#"\bvar(_os)?\("(HOME|USERPROFILE)"\)|\.home_dir\(\)"#).unwrap();
    assert!(home.is_match(r#"std::env::var_os("HOME")"#) && home.is_match("app.path().home_dir()") && !home.is_match("crate::platform::home_dir()"));
    let elsewhere: Vec<String> = by_path()
        .into_iter()
        .filter(|(rel, _)| rel != "platform.rs")
        .filter_map(|(rel, s)| {
            let line = s.code.lines().position(|l| !l.trim_start().starts_with("//") && home.is_match(l))?;
            Some(format!("{rel}:{}", line + 1))
        })
        .collect();
    assert_eq!(elsewhere, [] as [String; 0], "read the home folder with crate::platform::home_dir()");
}

/// Nothing the app runs listens beyond the loopback. Regression (#230): the port check bound
/// 0.0.0.0, and Windows Firewall asked the player whether Cyber CTF may accept connections from
/// public networks, mid-deploy.
#[test]
fn nothing_the_app_runs_listens_beyond_the_loopback() {
    for src in crate_sources() {
        for needle in ["\"0.0.0.0\"", "UNSPECIFIED", "\"::\"", "\"[::]"] {
            let binds = src.code.lines().filter(|l| l.contains("bind") && l.contains(needle)).collect::<Vec<_>>();
            assert_eq!(binds, [] as [&str; 0], "{} binds beyond the loopback", src.path.display());
        }
    }
}

/// Regression: build.rs read its own folder with `env!`, baked into the script's binary when
/// it was compiled. Cargo reuses that binary for every checkout sharing the target folder, so
/// once the checkout that built it was gone (a deleted worktree), every build failed reading
/// a Cargo.lock that no longer existed. Cargo's variables are read when the script runs.
#[test]
fn the_build_script_reads_cargos_variables_when_it_runs() {
    let build = include_str!("../../build.rs");
    let baked: Vec<&str> = build.lines().filter(|l| l.contains("env!(\"CARGO_")).collect();
    assert_eq!(baked, [] as [&str; 0], "build.rs reads these at compile time; use std::env::var_os");
}
