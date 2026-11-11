#!/usr/bin/env bash
# The desktop end-to-end, on this machine (Linux or macOS; Windows: run.ps1 beside this): the real
# app driven through its embedded WebDriver server, every spec with the needs it declares met
# first, one verdict per spec and one line per check, then the summary (tests/desktop/run.mjs runs
# the specs; this builds or unpacks the app and serves a debug build's frontend). Part of
# scripts/release/release-check.sh; the test rig's entry point (docs/testing/desktop-e2e.md says
# what each spec proves and needs).
#
# Safe on a machine someone uses: the app keeps all of its own in a folder made for the run
# (CYBERCTF_WDIO_ROOT: its data, settings, logs, webview data, session and secrets, never the
# user's), removed at the end; and every clean-up acts only on what the run made, by the records
# it wrote (tests/desktop/lib/owned.mjs).
#
#   tests/desktop/run.sh --all                                 # every spec (the default)
#   tests/desktop/run.sh --group lab                           # one group of specs (the rig's shards)
#   tests/desktop/run.sh --specs "smoke settings" --knobs ~/e2e.env
#   tests/desktop/run.sh --app path/to/cyberctf-desktop        # an app already built (no build here)
#   tests/desktop/run.sh --skip-build                          # the debug build as it is
#   tests/desktop/run.sh --package path/to/Cyber.CTF_1.8.9_amd64.deb   # or an .AppImage, an .rpm
#   tests/desktop/run.sh --results results.jsonl               # and a JSON record per spec and check
#   tests/desktop/run.sh --list                                # the cases, their knobs, groups and needs
#
# Options:
#   --all                 every spec (the default)
#   --group <name>        only that group's specs (--list says each case's groups)
#   --specs "<names>"     only these specs (as ONLY)
#   --app <binary>        the app to drive, built with `--features wdio` (CYBERCTF_TEST_DESKTOP_APP);
#                         one under target/release must come from `NEXT_PUBLIC_WDIO=1 pnpm tauri
#                         build --features wdio`, which embeds the frontend with the hooks (a
#                         `cargo build --release` doesn't)
#   --package <file>      a .deb, .rpm or .AppImage of such a build: unpacked here, not installed
#   --skip-build          use the debug build as it is (it loads devUrl, served here)
#   --knobs <file>        KEY=value lines (read literally, never sourced), exported first
#   --results <file>      append a JSON record per spec and per check to <file> (tests/README.md,
#                         "Results")
#   --list                print a JSON line per case (its id, spec, form, knobs, groups, needs) and exit
#
# Knobs (environment, or the knobs file; tests/README.md has each spec's):
#   CYBERCTF_TEST_DESKTOP_APP     the app (default: the wdio `cargo build`, built here)
#   CYBERCTF_TEST_LOCAL_LAB       the lab folder to import (default: a checkout of the Invoice Portal
#                                 API lab under infra/test-servers/.state/labs)
#   CYBERCTF_TEST_ACCOUNT_SESSION a signed-in session (a dev-session.json), placed in the run's folder
#                                 (its home/.cyberctf/), where the run's app reads it whatever the build
#   CYBERCTF_TEST_VM_PROVIDER     the hypervisor for the VM specs (libvirt, virtualbox, hyperv, qemu, utm)
#   CYBERCTF_TEST_CATALOGUE_LAB, CYBERCTF_TEST_CATALOGUE_VM_LAB, CYBERCTF_TEST_SERVER_ENV,
#   CYBERCTF_TEST_SERVER_LAB, CYBERCTF_TEST_BOX_DESKTOP, CYBERCTF_TEST_CLEANUP_TOOL: see each spec
#   CYBERCTF_TEST_DESKTOP_TIMEOUT seconds a spec may take (300; machine 480); the specs scale their waits to it
#   CYBERCTF_TEST_DESKTOP_KEEP    leave the fixture lab running and the run's folder in place at the end
#   TAURI_WEBDRIVER_PORT          the app's embedded WebDriver port (4445)
#   ONLY, SKIP, STRICT            as every suite runner (scripts/lib/suite.sh)
# Exit status: 0 nothing failed (and no skip under STRICT), 1 a spec failed, 2 the runner itself
# couldn't run.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../../scripts/lib/suite.sh
. "$HERE/scripts/lib/suite.sh"
suite_help "$0" "$@"

fail_now() { printf 'run.sh: %s\n' "$*" >&2; exit 2; }
RUNNER="$HERE/tests/desktop/run.mjs"

# Arguments: the app's are taken here, the rest passed on to the runner.
PACKAGE=""
SKIP_BUILD=""
PASS=()
while [ $# -gt 0 ]; do
  case $1 in
    --app) CYBERCTF_TEST_DESKTOP_APP=${2:?--app needs a path}; SKIP_BUILD=1; shift ;;
    --package) PACKAGE=${2:?--package needs a file}; SKIP_BUILD=1; shift ;;
    --skip-build) SKIP_BUILD=1 ;;
    --list) exec node "$RUNNER" ${PASS[@]+"${PASS[@]}"} "$@" ;;
    --group | --specs | --results | --plan) PASS+=("$1" "${2:?$1 needs a value}"); shift ;;
    --all) PASS+=("$1") ;;
    --knobs)
      [ -f "${2:-}" ] || fail_now "no knobs file: ${2:-}"
      while IFS= read -r line || [ -n "$line" ]; do
        case $line in '' | '#'*) continue ;; esac
        export "${line%%=*}=${line#*=}"
      done <"$2"
      shift
      ;;
    *) fail_now "unknown argument: $1 (see --help)" ;;
  esac
  shift
done

cd "$HERE" || exit 2
NATIVE_STATE="$HERE/tests/desktop/.state"
LOGS=${CYBERCTF_TEST_DESKTOP_LOGS:-$NATIVE_STATE/logs}
mkdir -p "$LOGS"
# Where a `cargo build` app loads its frontend from: tauri.conf.json's devUrl, compiled into the
# app (not a knob: another port would serve a frontend the app never loads). A `next dev` already
# listening there is reused.
DEV_URL_PORT=$(sed -n 's|.*"devUrl": *"http://[^:]*:\([0-9]*\)".*|\1|p' src-tauri/tauri.conf.json)
[ -n "$DEV_URL_PORT" ] || fail_now "no devUrl port in src-tauri/tauri.conf.json"

use_nvmrc_node "$HERE"
command -v pnpm >/dev/null || fail_now "pnpm is not installed"
[ -d node_modules/webdriverio ] || pnpm install --frozen-lockfile || fail_now "pnpm install failed"

# --- The app ---------------------------------------------------------------------------------
# A package is unpacked, not installed: the binary inside runs as it is.
if [ -n "$PACKAGE" ]; then
  [ -f "$PACKAGE" ] || fail_now "no package: $PACKAGE"
  UNPACKED="$NATIVE_STATE/package"
  rm -rf "$UNPACKED" && mkdir -p "$UNPACKED"
  case $PACKAGE in
    *.deb) dpkg-deb -x "$PACKAGE" "$UNPACKED" || fail_now "dpkg-deb couldn't unpack $PACKAGE" ;;
    *.rpm) (cd "$UNPACKED" && rpm2cpio "$PACKAGE" | cpio -idm --quiet) || fail_now "couldn't unpack $PACKAGE" ;;
    *.AppImage)
      cp "$PACKAGE" "$UNPACKED/" && chmod +x "$UNPACKED/$(basename "$PACKAGE")"
      # Without FUSE (a container, a minimal guest) an AppImage still runs, extracted first.
      export APPIMAGE_EXTRACT_AND_RUN=1
      ;;
    *) fail_now "not a package this runner unpacks (.deb, .rpm, .AppImage): $PACKAGE" ;;
  esac
  case $PACKAGE in
    *.AppImage) CYBERCTF_TEST_DESKTOP_APP="$UNPACKED/$(basename "$PACKAGE")" ;;
    *) CYBERCTF_TEST_DESKTOP_APP=$(find "$UNPACKED/usr/bin" -type f -perm -u+x | head -n 1) ;;
  esac
fi
# The app the specs drive (tests/desktop/lib/support.mjs reads it).
export CYBERCTF_TEST_DESKTOP_APP=${CYBERCTF_TEST_DESKTOP_APP:-$HERE/src-tauri/target/debug/cyberctf-desktop}
APP_BIN=$CYBERCTF_TEST_DESKTOP_APP
if [ -z "$SKIP_BUILD" ]; then
  say "cargo build --features wdio"
  cargo build --manifest-path src-tauri/Cargo.toml --features wdio || fail_now "the wdio build failed"
fi
[ -x "$APP_BIN" ] || fail_now "app binary not found or not executable: $APP_BIN"

# --- The frontend a `cargo build` app loads (devUrl); a `tauri build` one embeds it ----------
NEXT_PGID=""
stop_frontend() {
  if [ -n "$NEXT_PGID" ]; then
    kill -- "-$NEXT_PGID" 2>/dev/null
    NEXT_PGID=""
  fi
}
trap stop_frontend EXIT INT TERM
case "$APP_BIN" in
  */debug/*)
    if serves "$DEV_URL_PORT"; then
      say "frontend: reusing the dev server on port $DEV_URL_PORT"
    else
      say "frontend: next dev on port $DEV_URL_PORT (a cargo build app loads devUrl, not out/)"
      set -m
      pnpm exec next dev --hostname 127.0.0.1 --port "$DEV_URL_PORT" >"$LOGS/next-dev.log" 2>&1 &
      NEXT_PGID=$!
      set +m
      for _ in $(seq 1 90); do
        serves "$DEV_URL_PORT" && break
        sleep 2
      done
      serves "$DEV_URL_PORT" || { tail -20 "$LOGS/next-dev.log"; fail_now "next dev never answered on $DEV_URL_PORT"; }
    fi
    ;;
esac

# --- The specs ---------------------------------------------------------------------------------
export CYBERCTF_TEST_DESKTOP_LOGS="$LOGS"
node "$RUNNER" ${PASS[@]+"${PASS[@]}"}
