#!/usr/bin/env bash
# The gate before a release: every check CI runs on a pull request's code and workflows (ci.yml,
# web-coverage.yml, coverage.yml, e2e.yml, lint-infra.yml), but the Terraform templates'
# validation, which stays in CI: it needs Terraform and the providers' registry. Plus the layer CI
# cannot run (the native GUI e2e, which needs a desktop session, a real Docker engine and a
# hypervisor), in one command on a developer machine, with one line per step at the end. Steps
# keep going after a failure so one run reports everything that is wrong. A runner run as a step
# (other-platforms, e2e-native) that skipped some of its own steps is recorded `skip`, not `ok`.
# docs/testing/release-check.md says what each step proves.
#
#   scripts/release/release-check.sh                     the whole gate (pnpm release-check)
#   ONLY="unit-web e2e-mock" scripts/release/release-check.sh
#   SKIP="e2e-native" scripts/release/release-check.sh   what CI covers, without the native layer
#
# Steps, in order: prereqs, install, versions, docs, features, format, types, lint, build, quality, unit-web
# (with the coverage floors), unit-rust, bindings (the generated types the Rust tests rewrote are
# committed), docs-rust (rustdoc's intra-doc links), other-platforms (clippy for Linux and Windows, scripts/check/other-platforms.sh;
# needs rustup), coverage-rust (when cargo-llvm-cov is installed), deny (when cargo-deny is),
# audit, shellcheck and actionlint (when installed, as lint-infra.yml runs them), pins (every
# action pinned to a commit SHA), store-listings (the app's JSON and the Microsoft Store listing,
# as lint-infra.yml checks them; needs python3), e2e-mock (Playwright against the dev mock), e2e-native
# (tests/desktop/run.sh, which has its own knobs).
#
# Knobs:
#   RUST_LINES_FLOOR    the Rust line coverage floor (default: coverage.yml's, read from there)
#   DEV_URL_PORT        a dev server Playwright reuses when it answers there (default: 3206)
#   ONLY, SKIP, STRICT  as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"
cd "$ROOT" || exit 2
use_nvmrc_node "$ROOT"
# The line floor coverage.yml enforces, read from it: kept in one place.
RUST_LINES_FLOOR=${RUST_LINES_FLOOR:-$(sed -n 's/^ *RUST_LINES_FLOOR: *"\{0,1\}\([0-9.]*\)"\{0,1\} *$/\1/p' .github/workflows/coverage.yml)}
[ -n "$RUST_LINES_FLOOR" ] || { echo "no RUST_LINES_FLOOR in .github/workflows/coverage.yml" >&2; exit 2; }
NODE_WANTED=$(tr -d '[:space:]' <.nvmrc)

prereqs() {
  local missing=0 tool
  for tool in node pnpm cargo; do
    command -v "$tool" >/dev/null || { echo "missing: $tool"; missing=1; }
  done
  # Docker only for the native layer (SKIP=e2e-native runs what CI runs, without it).
  if wanted e2e-native; then command -v docker >/dev/null || { echo "missing: docker (or SKIP=e2e-native)"; missing=1; }; fi
  node --version | grep -q "^v${NODE_WANTED}\." || { echo "node $(node --version) in use; .nvmrc asks for $NODE_WANTED (nvm install $NODE_WANTED)"; missing=1; }
  cargo machete --version >/dev/null 2>&1 || { echo "missing: cargo-machete (cargo install cargo-machete); pnpm quality needs it"; missing=1; }
  if wanted e2e-native && ! docker info >/dev/null 2>&1; then echo "note: docker is not running; e2e-native skips the scenarios that run a lab"; fi
  [ "$missing" -eq 0 ]
}
# has <command...>: whether the tool answers --version (a cargo subcommand that isn't installed doesn't).
has() { "$@" --version >/dev/null 2>&1; }
# The command types the Rust tests rewrite (ts_bindings.rs) are the ones committed, as ci.yml checks.
bindings_committed() {
  local changed
  changed=$(git status --porcelain -- src/lib/tauri/generated)
  [ -z "$changed" ] || { echo "$changed"; echo "src/lib/tauri/generated differs from the Rust structs: commit what cargo test wrote"; return 1; }
}
format_check() {
  pnpm format:check && cargo fmt --manifest-path src-tauri/Cargo.toml --check
}
rust_cov() {
  cargo llvm-cov --no-report --manifest-path src-tauri/Cargo.toml &&
    cargo llvm-cov report --manifest-path src-tauri/Cargo.toml --summary-only --fail-under-lines "$RUST_LINES_FLOOR"
}
# Next's dev lock allows one dev server per checkout: when one already serves the app's devUrl
# port (`pnpm tauri dev`, or e2e-native's), Playwright runs against it instead of starting its own.
e2e_mock() {
  if serves "${DEV_URL_PORT:-3206}"; then
    echo "reusing the dev server on port ${DEV_URL_PORT:-3206}"
    CYBERCTF_TEST_WEB_PORT=${DEV_URL_PORT:-3206} pnpm test:web
  else
    pnpm test:web
  fi
}

step prereqs prereqs
step install pnpm install --frozen-lockfile
step versions pnpm check:tauri
step docs pnpm check:docs
step features pnpm check:features
step format format_check
step types pnpm typecheck
step lint pnpm lint
step build pnpm build
step quality pnpm quality
# The coverage run applies vitest.config.mts's floors, as web-coverage.yml does.
step unit-web pnpm exec vitest run --coverage
step unit-rust cargo test --manifest-path src-tauri/Cargo.toml
step bindings bindings_committed
# Every intra-doc link resolves on this machine's items, as ci.yml's Docs step (the other
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
# lint-infra.yml's checks of the files around the app.
if command -v shellcheck >/dev/null; then step shellcheck scripts/check/infra.sh shellcheck; else skip shellcheck "shellcheck not installed (https://www.shellcheck.net)"; fi
if command -v actionlint >/dev/null; then step actionlint actionlint -color; else skip actionlint "actionlint not installed (https://github.com/rhysd/actionlint)"; fi
step pins scripts/check/infra.sh pins
if command -v python3 >/dev/null; then
  step store-listings python3 .github/scripts/check-store-listings.py
else
  skip store-listings "python3 not installed"
fi
step e2e-mock e2e_mock
case "$(uname -s)" in
  Darwin | Linux) nested e2e-native tests/desktop/run.sh ;;
  *) skip e2e-native "run tests/desktop/run.ps1 on Windows" ;;
esac

suite_end "release check"
