//! What the unit tests share: an app on Tauri's mock runtime, the property tests' strategies, the
//! scans over the crate's own source, the warnings the code logs, and the TypeScript types written
//! from the command structs.

pub mod logs;
pub mod proptest_support;
pub mod source_scan;
pub mod test_app;
mod ts_bindings;

/// Asserts that `actual` is `expected` as a float can be: within a millionth of it (an exact
/// comparison of floats says more about rounding than about the code under test).
#[track_caller]
pub fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} is not {expected}");
}
