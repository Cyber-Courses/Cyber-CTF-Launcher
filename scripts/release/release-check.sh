#!/usr/bin/env bash
# The launcher's check: every check its code gets, in one command, with one line per step at the
# end. CI runs nothing else (.github/workflows/ci.yml runs the short list, FAST=1, on pull requests
# into dev; gate.yml runs every step before main and before a release, each job a few steps with
# ONLY). Plus the layer CI cannot run, the native GUI e2e (a desktop session, a real Docker engine
# and a hypervisor), which the workspace's rig machines run. Steps keep going after a failure so
# one run reports everything that is wrong. A runner run as a step (other-platforms, e2e-native)
# that skipped some of its own steps is recorded `skip`, not `ok`.
# docs/contributing/testing/release-check.md says what each step proves.
#
#   scripts/release/release-check.sh                     every step (pnpm release-check)
#   SKIP="e2e-native" scripts/release/release-check.sh   all but the native layer (pnpm check)
#   FAST=1 scripts/release/release-check.sh              the short list (pnpm check:fast)
#   ONLY="unit-web e2e-mock" scripts/release/release-check.sh
#
# Steps, in order: prereqs, install, versions, docs, features, format, clippy (this machine's),
# types, lint, build, quality, unit-web (with the coverage floors), unit-harness (the test runners'
# own tests), unit-rust (cargo-nextest, each test in its own process, then the doctests; each unit
# step fails on what its tests leave in their temp folder, scripts/check/temp-leaks.ts), bindings (the generated types the Rust tests rewrote are
# committed), docs-rust (rustdoc's intra-doc links), other-platforms (clippy and rustdoc for Linux
# and Windows, scripts/check/other-platforms.sh; needs rustup), coverage-rust (when cargo-llvm-cov
# is installed), deny (when cargo-deny is), audit, shellcheck and actionlint (when installed),
# hooks (the git hooks' own test), pins (every action pinned to a commit SHA), store-listings (the
# app's JSON and the Microsoft Store listing; needs python3), e2e-mock (Playwright against the dev
# mock), e2e-native (tests/e2e/desktop/runner/run.sh, which has its own knobs).
#
# Knobs:
#   FAST=1              the short list (FAST_STEPS below), unless ONLY names steps
#   RUST_LINES_FLOOR    the Rust line coverage floor (default: RUST_LINES_FLOOR below)
#   ONLY, SKIP, STRICT  as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"
cd "$ROOT" || exit 2
use_nvmrc_node "$ROOT"
# sccache as the Rust compiler's wrapper when it is installed (never required): a crate another
# worktree or target folder compiled comes from its cache. A RUSTC_WRAPPER already set, empty
# included, is left as it is.
if [ -z "${RUSTC_WRAPPER+set}" ] && command -v sccache >/dev/null; then export RUSTC_WRAPPER=sccache; fi
# The Rust line coverage floor in percent: about a point under what dev measures, raised as it
# rises (82.7 until the opt-in integration tests were left out of the measure; the app's own code
# measured 87.30% on macOS, 2026-11-16).
RUST_LINES_FLOOR=${RUST_LINES_FLOOR:-86.3}
# The files the coverage leaves out: the opt-in integration tests, which run only against a real
# Docker engine, hypervisor or server (test code, not the app's).
RUST_COV_IGNORE="src/integration_tests/"
# The short list while you work (pnpm check:fast, and ci.yml on pull requests into dev): what a
# change most often breaks, in a few minutes.
FAST_STEPS="prereqs install format clippy unit-rust other-platforms types lint unit-web"
if [ -n "${FAST:-}" ] && [ -z "${ONLY:-}" ]; then ONLY=$FAST_STEPS; fi
NODE_WANTED=$(tr -d '[:space:]' <.nvmrc)

prereqs() {
  local missing=0 tool
  for tool in node pnpm cargo; do
    command -v "$tool" >/dev/null || { echo "missing: $tool"; missing=1; }
  done
  # Docker only for the native layer (SKIP=e2e-native runs what CI runs, without it).
  if wanted e2e-native; then command -v docker >/dev/null || { echo "missing: docker (or SKIP=e2e-native)"; missing=1; }; fi
  node --version | grep -q "^v${NODE_WANTED}\." || { echo "node $(node --version) in use; .nvmrc asks for $NODE_WANTED (nvm install $NODE_WANTED)"; missing=1; }
  if wanted unit-rust || wanted coverage-rust; then cargo nextest --version >/dev/null 2>&1 || { echo "missing: cargo-nextest (cargo install --locked cargo-nextest)"; missing=1; }; fi
  # The Rust contract tests run Isoloom's own example labs (spec_run.rs), read from a checkout.
  local isoloom=${CYBERCTF_TEST_ISOLOOM_DIR:-$ROOT/../isoloom}
  if { wanted unit-rust || wanted coverage-rust; } && [ ! -d "$isoloom/examples" ]; then
    echo "missing: an Isoloom checkout at $isoloom, for the Rust contract tests (set CYBERCTF_TEST_ISOLOOM_DIR; the workspace's is ../isoloom)"
    missing=1
  fi
  if wanted quality; then cargo machete --version >/dev/null 2>&1 || { echo "missing: cargo-machete (cargo install cargo-machete); pnpm quality needs it"; missing=1; }; fi
  if wanted e2e-native && ! docker info >/dev/null 2>&1; then echo "note: docker is not running; e2e-native skips the scenarios that run a lab"; fi
  [ "$missing" -eq 0 ]
}
# has <command...>: whether the tool answers --version (a cargo subcommand that isn't installed doesn't).
has() { "$@" --version >/dev/null 2>&1; }
# The command types the Rust tests rewrite (ts_bindings.rs) are the ones committed.
bindings_committed() {
  local changed
  changed=$(git status --porcelain -- src/lib/tauri/generated)
  [ -z "$changed" ] || { echo "$changed"; echo "src/lib/tauri/generated differs from the Rust structs: commit what cargo test wrote"; return 1; }
}
format_check() {
  pnpm format:check && cargo fmt --manifest-path src-tauri/Cargo.toml --check
}
# The Rust tests under cargo-nextest (src-tauri/.config/nextest.toml: each test in its own process,
# nothing retried, a slow test reported), then the doctests, which nextest doesn't run. Both run,
# so a failure in one doesn't hide the other.
rust_tests() {
  local status=0
  cargo nextest run --manifest-path src-tauri/Cargo.toml || status=$?
  cargo test --manifest-path src-tauri/Cargo.toml --doc || status=$?
  return "$status"
}
rust_cov() {
  cargo llvm-cov nextest --no-report --manifest-path src-tauri/Cargo.toml &&
    cargo llvm-cov report --manifest-path src-tauri/Cargo.toml --ignore-filename-regex "$RUST_COV_IGNORE" --summary-only --fail-under-lines "$RUST_LINES_FLOOR"
}

step prereqs prereqs
step install pnpm install --frozen-lockfile
step versions pnpm check:tauri
step docs pnpm check:docs
step features pnpm check:features
step format format_check
# This machine's clippy, every lint at the level Cargo.toml's [lints] sets (other-platforms runs
# the other systems').
step clippy cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
step types pnpm typecheck
step lint pnpm lint
step build pnpm build
step quality pnpm quality
# The coverage run applies vitest.config.mts's floors.
# The unit tests run with a temp folder of their own: what they leave there fails the step.
step unit-web pnpm check:temp-leaks pnpm exec vitest run --coverage
step unit-harness pnpm test:harness
step unit-rust pnpm check:temp-leaks bash -c "$(declare -f rust_tests); rust_tests"
step bindings bindings_committed
# Every intra-doc link resolves on this machine's items (the other
# platforms' are other-platforms' -docs steps).
step docs-rust env RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D rustdoc::invalid_html_tags" cargo doc --manifest-path src-tauri/Cargo.toml --no-deps --document-private-items --lib
# CI's clippy matrix for the other platforms, from here: the code behind their `#[cfg]`.
if command -v rustup >/dev/null; then nested other-platforms scripts/check/other-platforms.sh; else skip other-platforms "rustup not installed (https://rustup.rs)"; fi
if has cargo llvm-cov; then step coverage-rust rust_cov; else skip coverage-rust "cargo-llvm-cov not installed (cargo install cargo-llvm-cov)"; fi
if has cargo deny; then
  step deny cargo deny --manifest-path src-tauri/Cargo.toml check advisories bans licenses sources
else
  skip deny "cargo-deny not installed (cargo install cargo-deny)"
fi
step audit pnpm audit --prod
# The checks of the files around the app.
if command -v shellcheck >/dev/null; then step shellcheck scripts/check/infra.sh shellcheck; else skip shellcheck "shellcheck not installed (https://www.shellcheck.net)"; fi
if command -v actionlint >/dev/null; then step actionlint actionlint -color; else skip actionlint "actionlint not installed (https://github.com/rhysd/actionlint)"; fi
# The git hooks, in a throwaway repository (.githooks/test.sh).
if command -v gitleaks >/dev/null; then step hooks .githooks/test.sh; else skip hooks "gitleaks not installed (brew install gitleaks)"; fi
step pins scripts/check/infra.sh pins
if command -v python3 >/dev/null; then
  step store-listings python3 .github/scripts/check-store-listings.py
else
  skip store-listings "python3 not installed"
fi
step e2e-mock pnpm test:web
case "$(uname -s)" in
  Darwin | Linux) nested e2e-native tests/e2e/desktop/runner/run.sh ;;
  *) skip e2e-native "run tests/e2e/desktop/runner/run.ps1 on Windows" ;;
esac

suite_end "release check"
