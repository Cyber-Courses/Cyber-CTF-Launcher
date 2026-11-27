#!/usr/bin/env bash
# The Rust tests, as release-check.sh's unit-rust step runs them (through
# scripts/check/temp-leaks.ts): under cargo-nextest (src-tauri/.config/nextest.toml: each test in
# its own process, nothing retried, a slow test reported), then the doctests, which nextest doesn't
# run. Both run, so a failure in one doesn't hide the other. A file of its own, not a function
# handed to `bash -c`: on Windows pnpm and cmd turn that function's new lines into `\n`.
#
#   scripts/check/rust-tests.sh
#
# Run from anywhere; exits with the first failing command's status.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 2
status=0
cargo nextest run --manifest-path src-tauri/Cargo.toml || status=$?
cargo test --manifest-path src-tauri/Cargo.toml --doc || status=$?
exit "$status"
