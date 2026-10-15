//! What the unit tests share: an app on Tauri's mock runtime, the property tests' strategies, the
//! scans over the crate's own source, and the TypeScript types written from the command structs.

pub mod proptest_support;
pub mod source_scan;
pub mod test_app;
mod ts_bindings;
