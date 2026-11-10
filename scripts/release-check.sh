#!/usr/bin/env bash
# The gate before a release: every check CI runs, plus the layer it cannot (the native GUI e2e,
# which needs a desktop session, a real Docker engine and a hypervisor), in one command on a
# developer machine, with one line per step at the end. Steps keep going after a failure so one
# run reports everything that is wrong. docs/testing/release-check.md says what each step proves.
#
#   scripts/release-check.sh                     the whole gate (pnpm release-check)
#   ONLY="unit-web e2e-mock" scripts/release-check.sh
#   SKIP="e2e-native" scripts/release-check.sh   what CI covers, without the native layer
#
# Steps, in order: prereqs, install, versions, docs, format, types, lint, build, quality, unit-web
# (with the coverage floors), unit-rust, bindings (the generated types the Rust tests rewrote are
# committed), coverage-rust (when cargo-llvm-cov is installed), deny (when cargo-deny is), audit,
# e2e-mock (Playwright against the dev mock), e2e-native (e2e/desktop/run.sh, which has its own
# knobs).
#
# Knobs:
#   RUST_LINES_FLOOR    the Rust line coverage floor (default: coverage.yml's)
#   DEV_URL_PORT        a dev server Playwright reuses when it answers there (default: 3000)
#   ONLY, SKIP, STRICT  as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"
cd "$ROOT" || exit 2
use_nvmrc_node "$ROOT"
# The line floor coverage.yml enforces; kept in one place there, mirrored here.
RUST_LINES_FLOOR=${RUST_LINES_FLOOR:-82.7}
NODE_WANTED=$(tr -d '[:space:]' <.nvmrc)

prereqs() {
  local missing=0 tool
  for tool in node pnpm cargo docker; do
    command -v "$tool" >/dev/null || { echo "missing: $tool"; missing=1; }
  done
  node --version | grep -q "^v${NODE_WANTED}\." || { echo "node $(node --version) in use; .nvmrc asks for $NODE_WANTED (nvm install $NODE_WANTED)"; missing=1; }
  cargo machete --version >/dev/null 2>&1 || { echo "missing: cargo-machete (cargo install cargo-machete); pnpm quality needs it"; missing=1; }
  docker info >/dev/null 2>&1 || echo "note: docker is not running; e2e-native skips the scenarios that run a lab"
  cargo llvm-cov --version >/dev/null 2>&1 || echo "note: cargo-llvm-cov is not installed; coverage-rust is skipped (cargo install cargo-llvm-cov)"
  cargo deny --version >/dev/null 2>&1 || echo "note: cargo-deny is not installed; deny is skipped (cargo install cargo-deny)"
  [ "$missing" -eq 0 ]
}
# The command types the Rust tests rewrite (ts_bindings.rs) are the ones committed, as ci.yml checks.
bindings_committed() {
  local changed
  changed=$(git status --porcelain -- src/lib/tauri/generated)
  [ -z "$changed" ] || { echo "$changed"; echo "src/lib/tauri/generated differs from the Rust structs: commit what cargo test wrote"; return 1; }
}
format_check() {
  pnpm exec prettier --check "{src,scripts,e2e}/**/*.{ts,tsx,css,mjs}" "{*,docs/**/*,infra/**/*,scripts/**/*}.md" && cargo fmt --manifest-path src-tauri/Cargo.toml --check
}
rust_cov() {
  cargo llvm-cov --no-report --manifest-path src-tauri/Cargo.toml &&
    cargo llvm-cov report --manifest-path src-tauri/Cargo.toml --summary-only --fail-under-lines "$RUST_LINES_FLOOR"
}
# Next's dev lock allows one dev server per checkout: when one already serves the app's devUrl
# port (`pnpm tauri dev`, or e2e-native's), Playwright runs against it instead of starting its own.
e2e_mock() {
  if curl -fsS -o /dev/null "http://127.0.0.1:${DEV_URL_PORT:-3000}/"; then
    echo "reusing the dev server on port ${DEV_URL_PORT:-3000}"
    E2E_PORT=${DEV_URL_PORT:-3000} pnpm exec playwright test
  else
    pnpm exec playwright test
  fi
}

step prereqs prereqs
step install pnpm install --frozen-lockfile
step versions pnpm check:tauri
step docs pnpm check:docs
step format format_check
step types pnpm typecheck
step lint pnpm lint
step build pnpm build
step quality pnpm quality
# The coverage run applies vitest.config.mts's floors, as web-coverage.yml does.
step unit-web pnpm exec vitest run --coverage
step unit-rust cargo test --manifest-path src-tauri/Cargo.toml
step bindings bindings_committed
if cargo llvm-cov --version >/dev/null 2>&1; then step coverage-rust rust_cov; else skip coverage-rust "cargo-llvm-cov not installed"; fi
if cargo deny --version >/dev/null 2>&1; then
  step deny cargo deny --manifest-path src-tauri/Cargo.toml check advisories bans licenses sources
else
  skip deny "cargo-deny not installed"
fi
step audit pnpm audit --prod
step e2e-mock e2e_mock
case "$(uname -s)" in
  Darwin | Linux) step e2e-native e2e/desktop/run.sh ;;
  *) skip e2e-native "run e2e/desktop/smoke.ps1 on Windows" ;;
esac

suite_end "release check"
