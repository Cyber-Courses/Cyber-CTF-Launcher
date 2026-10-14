//! Size limits on the Rust sources (#150): about 400 lines of code per file, the part before a
//! file's inline `#[cfg(test)]` module (function length is clippy's `too_many_lines`, set in
//! `clippy.toml`). Test-only files don't count. A file over the limit is split into submodules,
//! or its inline tests move to a `tests.rs` beside it.

use crate::source_scan::crate_sources;

/// Lines of code a source file may have before its tests.
const MAX_FILE_LINES: usize = 400;

/// Files still over the limit, to split in a follow-up. A file leaves the list when it is split
/// (the test fails while it's listed but under the limit).
const OVER_THE_LIMIT: &[&str] = &["account/agent.rs", "deploy_worker/process.rs", "exec/elevate.rs", "machine/system/wsl.rs", "runtime/cloud_services.rs"];

/// The ignored real-lab tests: a test-only module without an inline `mod tests`.
const TEST_ONLY: &[&str] = &["runtime/real_labs.rs"];

/// Each source file (its path relative to `src`, with `/`) and its lines of code before the tests.
fn code_lines() -> Vec<(String, usize)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    crate_sources()
        .into_iter()
        .map(|s| (s.path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"), s.code))
        .filter(|(rel, _)| !TEST_ONLY.contains(&rel.as_str()))
        .map(|(rel, code)| (rel, code.lines().count()))
        .collect()
}

#[test]
fn source_files_stay_under_the_size_limit() {
    let sizes = code_lines();
    assert!(sizes.len() > 100, "{} source files", sizes.len());
    for (rel, lines) in &sizes {
        let listed = OVER_THE_LIMIT.contains(&rel.as_str());
        assert!(
            listed || *lines <= MAX_FILE_LINES,
            "src/{rel} has {lines} lines of code (over {MAX_FILE_LINES}): split it into submodules, or move its tests to a tests.rs"
        );
        assert!(!listed || *lines > MAX_FILE_LINES, "src/{rel} is down to {lines} lines: remove it from OVER_THE_LIMIT");
    }
    for rel in OVER_THE_LIMIT {
        assert!(sizes.iter().any(|(r, _)| r == rel), "OVER_THE_LIMIT names src/{rel}, which isn't there");
    }
}
