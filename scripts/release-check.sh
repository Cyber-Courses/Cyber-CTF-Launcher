#!/usr/bin/env bash
# The gate before a release: every check CI runs, plus the layer it cannot (the native GUI e2e,
# which needs a desktop session, a real Docker engine and a hypervisor), in one command on a
# developer machine, with one line per step at the end. Steps keep going after a failure so one
# run reports everything that is wrong.
#
#   scripts/release-check.sh                    # the whole gate
#   ONLY="unit-web e2e-mock" scripts/release-check.sh
#   SKIP="e2e-native" scripts/release-check.sh  # what CI covers, without the native layer
#
# Steps, in order: prereqs, install, versions, format, types, lint, quality, unit-web (with the
# coverage floors), unit-rust, coverage-rust (when cargo-llvm-cov is installed), e2e-mock
# (Playwright against the dev mock), e2e-native (tests/native/run-all.sh, which documents its knobs).
# docs/release-check.md says what each step proves and what stays manual.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/.." && pwd)
cd "$HERE" || exit 2
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.cargo/bin:/Applications/Docker.app/Contents/Resources/bin"
# The line floor coverage.yml enforces; kept in one place there, mirrored here.
RUST_LINES_FLOOR=${RUST_LINES_FLOOR:-82.7}

RESULTS=()
FAILED=0
wanted() {
  if [ -n "${ONLY:-}" ] && [[ " $ONLY " != *" $1 "* ]]; then return 1; fi
  if [ -n "${SKIP:-}" ] && [[ " $SKIP " == *" $1 "* ]]; then return 1; fi
  return 0
}
record() { RESULTS+=("$(printf '%-14s %-7s %5ss  %s' "$1" "$2" "$3" "$4")"); }
# step <name> <command...>: runs it in the open (its output is the detail), records the verdict.
step() {
  local name=$1
  shift
  wanted "$name" || return 0
  printf '\n==== %s: %s\n' "$name" "$*"
  local start=$SECONDS
  if "$@"; then
    record "$name" ok $((SECONDS - start)) ""
  else
    local rc=$?
    FAILED=$((FAILED + 1))
    record "$name" FAIL $((SECONDS - start)) "exit $rc"
  fi
}
skip() {
  wanted "$1" || return 0
  printf '\n==== %s: skipped (%s)\n' "$1" "$2"
  record "$1" skip 0 "$2"
}

# --- Node: the version .nvmrc names ---------------------------------------------------------
want=$(tr -d '[:space:]' < .nvmrc)
if ! node --version 2>/dev/null | grep -q "^v${want}\."; then
  nvm_sh="${NVM_DIR:-$HOME/.nvm}/nvm.sh"
  if [ -s "$nvm_sh" ]; then
    set +u
    # shellcheck source=/dev/null
    . "$nvm_sh"
    nvm use "$want" >/dev/null 2>&1
    set -u
  fi
fi

prereqs() {
  local missing=0
  local tool
  for tool in node pnpm cargo docker; do
    command -v "$tool" >/dev/null || { echo "missing: $tool"; missing=1; }
  done
  node --version | grep -q "^v${want}\." || { echo "node $(node --version) in use; .nvmrc asks for $want (nvm install $want)"; missing=1; }
  cargo machete --version >/dev/null 2>&1 || { echo "missing: cargo-machete (cargo install cargo-machete); pnpm quality needs it"; missing=1; }
  docker info >/dev/null 2>&1 || echo "note: docker is not running; e2e-native skips the scenarios that run a lab"
  cargo llvm-cov --version >/dev/null 2>&1 || echo "note: cargo-llvm-cov is not installed; coverage-rust is skipped (cargo install cargo-llvm-cov)"
  [ "$missing" -eq 0 ]
}

step prereqs prereqs
step install pnpm install --frozen-lockfile
step versions pnpm check:tauri
format_check() { pnpm exec prettier --check "{src,scripts}/**/*.{ts,tsx,css}" && cargo fmt --manifest-path src-tauri/Cargo.toml --check; }
step format format_check
step types pnpm typecheck
step lint pnpm lint
step quality pnpm quality
# The coverage run applies vitest.config.mts's floors, as web-coverage.yml does.
step unit-web pnpm exec vitest run --coverage
step unit-rust cargo test --manifest-path src-tauri/Cargo.toml
if cargo llvm-cov --version >/dev/null 2>&1; then
  rust_cov() {
    cargo llvm-cov --no-report --manifest-path src-tauri/Cargo.toml \
      && cargo llvm-cov report --manifest-path src-tauri/Cargo.toml --summary-only --fail-under-lines "$RUST_LINES_FLOOR"
  }
  step coverage-rust rust_cov
else
  skip coverage-rust "cargo-llvm-cov not installed"
fi
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
step e2e-mock e2e_mock
case "$(uname -s)" in
  Darwin | Linux) step e2e-native tests/native/run-all.sh ;;
  *) skip e2e-native "run tests/native/run-embedded.ps1 on Windows" ;;
esac

printf '\n==== release check\n'
printf '   %s\n' "${RESULTS[@]}"
if [ "$FAILED" -gt 0 ]; then
  printf '\n%d step(s) failed: not ready to release.\n' "$FAILED"
  exit 1
fi
printf '\nAll steps passed.\n'
