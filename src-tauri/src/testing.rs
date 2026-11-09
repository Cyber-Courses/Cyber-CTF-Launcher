//! What the unit tests share: an app on Tauri's mock runtime, the property tests' strategies, the
//! scans over the crate's own source, the warnings the code logs, and the TypeScript types written
//! from the command structs.

pub mod logs;
pub mod proptest_support;
pub mod source_scan;
pub mod test_app;
mod ts_bindings;

/// A fresh folder under the temp dir for one test, named after `tag`.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cyberctf-{tag}-{}-{}", std::process::id(), rand::random::<u32>()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Asserts that `actual` is `expected` as a float can be: within a millionth of it (an exact
/// comparison of floats says more about rounding than about the code under test).
#[track_caller]
pub fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} is not {expected}");
}
