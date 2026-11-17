//! The contract tests between the layers: each drives both sides of one seam for real, Isothrum's
//! fake processes the only fake, so a change on either side fails here rather than in a lab
//! (tests/README.md, "Contract tests between the layers"). Each seam is a module:
//!
//! - `service`: the launcher and Isothrum's runner service, over the wire.
//!
//! `cargo test --manifest-path src-tauri/Cargo.toml --lib isothrum::tests::contract` runs them.

mod service;
