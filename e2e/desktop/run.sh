#!/usr/bin/env bash
# The desktop end-to-end, on this machine (Linux or macOS; Windows: run.ps1 beside this): the real
# app driven through its embedded WebDriver server, every spec in an order that leaves nothing
# behind, one verdict per spec and one line per check, then the summary. Each spec skips, saying
# why, when a knob it needs is missing (a lab folder, a hypervisor, a server, a signed-in
# session), so the whole set runs anywhere and reports pass, fail or skip honestly. Part of
# scripts/release-check.sh; the test rig's entry point (docs/testing/desktop-e2e.md says what each
# spec proves and needs).
#
#   e2e/desktop/run.sh --all                                 # every spec (the default)
#   e2e/desktop/run.sh --specs "smoke settings" --knobs ~/.config/cyberctf-test/e2e.env
#   e2e/desktop/run.sh --app path/to/cyberctf-desktop        # an app already built (no build here)
#   e2e/desktop/run.sh --package path/to/Cyber.CTF_1.8.9_amd64.deb   # or an .AppImage, an .rpm
#
# Options:
#   --all                 every spec (the default)
#   --specs "<names>"     only these specs (as ONLY)
#   --app <binary>        the app to drive, built with `--features wdio` (APP_BIN)
#   --package <file>      a .deb, .rpm or .AppImage of such a build: unpacked here, not installed
#   --knobs <file>        KEY=value lines (read literally, never sourced), exported first
#
# Knobs (environment, or the knobs file):
#   APP_BIN                     the app (default: the wdio `cargo build`, built here unless SKIP_BUILD,
#                               which loads devUrl); one under target/release must come from
#                               `NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`, which embeds the
#                               frontend with the hooks (a `cargo build --release` doesn't)
#   CYBERCTF_TEST_LOCAL_LAB     the lab folder to import (default: a checkout of the Invoice Portal
#                               API lab under infra/test-servers/.state/labs)
#   CYBERCTF_TEST_SESSION       a signed-in session (a debug build's dev-session.json), placed at
#                               ~/.cyberctf/dev-session.json for the run (the one there is put back)
#   CYBERCTF_TEST_VM_PROVIDER   the hypervisor for the VM specs (libvirt, virtualbox, hyperv, qemu, utm)
#   CYBERCTF_TEST_CATALOGUE_LAB, CYBERCTF_TEST_CATALOGUE_VM_LAB, CYBERCTF_TEST_SERVER_ENV,
#   CYBERCTF_TEST_SERVER_LAB, CYBERCTF_TEST_ATTACK_DESKTOP, CYBERCTF_TEST_REMOVE_TOOL: see each spec
#   TAURI_WEBDRIVER_PORT        the embedded WebDriver port (4445)
#   DEV_URL_PORT                where a debug build loads its frontend (3206; a dev server there is reused)
#   CASE_TIMEOUT                seconds for a spec without a lab in a VM (1800; the VM ones get 3 h)
#   KEEP                        leave the fixture lab running at the end
#   ONLY, SKIP, STRICT          as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

HERE=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../../scripts/lib/suite.sh
. "$HERE/scripts/lib/suite.sh"
suite_help "$0" "$@"

fail_now() { printf 'run.sh: %s\n' "$*" >&2; exit 2; }

# Arguments: everything else stays a knob.
PACKAGE=""
while [ $# -gt 0 ]; do
  case $1 in
    --all) ONLY="" ;;
    --specs) ONLY=${2:?--specs needs names}; shift ;;
    --app) APP_BIN=${2:?--app needs a path}; SKIP_BUILD=1; shift ;;
    --package) PACKAGE=${2:?--package needs a file}; SKIP_BUILD=1; shift ;;
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
export ONLY="${ONLY:-}"

cd "$HERE" || exit 2
NATIVE_STATE="$HERE/e2e/desktop/.state"
LOGS="$NATIVE_STATE/logs"
mkdir -p "$LOGS"
CASE_TIMEOUT=${CASE_TIMEOUT:-1800}
VM_TIMEOUT=10800
DEV_URL_PORT=${DEV_URL_PORT:-3206}
export TAURI_WEBDRIVER_PORT=${TAURI_WEBDRIVER_PORT:-4445}
OS=$(uname -s)

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
    *.AppImage) APP_BIN="$UNPACKED/$(basename "$PACKAGE")" ;;
    *) APP_BIN=$(find "$UNPACKED/usr/bin" -type f -perm -u+x | head -n 1) ;;
  esac
fi
APP_BIN=${APP_BIN:-$HERE/src-tauri/target/debug/cyberctf-desktop}
export APP_BIN
if [ -z "${SKIP_BUILD:-}" ]; then
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
case "$APP_BIN" in
  */debug/*)
    if serves "$DEV_URL_PORT"; then
      say "frontend: reusing the dev server on port $DEV_URL_PORT"
    else
      say "frontend: next dev on port $DEV_URL_PORT (a debug build loads devUrl, not out/)"
      set -m
      pnpm exec next dev -p "$DEV_URL_PORT" >"$LOGS/next-dev.log" 2>&1 &
      NEXT_PGID=$!
      set +m
      for _ in $(seq 1 90); do
        serves "$DEV_URL_PORT" && break
        sleep 2
      done
      serves "$DEV_URL_PORT" || { tail -20 "$LOGS/next-dev.log"; stop_frontend; fail_now "next dev never answered on $DEV_URL_PORT"; }
    fi
    ;;
esac

# --- A signed-in session, for the specs that start catalogue labs ------------------------------
# A debug build keeps its session in ~/.cyberctf/dev-session.json (a release build: the system's
# keychain, which can't be seeded: those specs then skip, saying the app isn't signed in).
SESSION_FILE="$HOME/.cyberctf/dev-session.json"
SESSION_BACKUP=""
SESSION_PLACED=""
place_session() {
  [ -n "${CYBERCTF_TEST_SESSION:-}" ] || return 0
  [ -f "$CYBERCTF_TEST_SESSION" ] || fail_now "no session file: $CYBERCTF_TEST_SESSION"
  case "$APP_BIN" in */debug/*) ;; *) say "session: a release build reads the keychain, CYBERCTF_TEST_SESSION is ignored"; return 0 ;; esac
  mkdir -p "$(dirname "$SESSION_FILE")"
  if [ -f "$SESSION_FILE" ]; then
    SESSION_BACKUP="$NATIVE_STATE/dev-session.backup.json"
    cp -p "$SESSION_FILE" "$SESSION_BACKUP"
  fi
  (umask 077 && cp "$CYBERCTF_TEST_SESSION" "$SESSION_FILE")
  SESSION_PLACED=1
  say "session: placed $SESSION_FILE for the run"
}
restore_session() {
  [ -n "$SESSION_PLACED" ] || return 0
  if [ -n "$SESSION_BACKUP" ]; then mv -f "$SESSION_BACKUP" "$SESSION_FILE"; else rm -f "$SESSION_FILE"; fi
  SESSION_PLACED=""
}
place_session

# --- Fixtures ---------------------------------------------------------------------------------
DOCKER_OK=1
docker info >/dev/null 2>&1 || DOCKER_OK=0
if [ -z "${CYBERCTF_TEST_LOCAL_LAB:-}" ]; then
  # The same lab the server tests deploy, checked out under infra/test-servers/.state (ignored).
  SERVER_TEST="$HERE/infra/test-servers"
  # shellcheck source=infra/test-servers/lib.sh
  . "$SERVER_TEST/lib.sh"
  CYBERCTF_TEST_LOCAL_LAB=$(lab_dir) || fail_now "could not check out the fixture lab"
fi
export CYBERCTF_TEST_LOCAL_LAB
[ -f "$CYBERCTF_TEST_LOCAL_LAB/isoloom.yml" ] || fail_now "not a lab folder: $CYBERCTF_TEST_LOCAL_LAB"
lab_name() { sed -n 's/^name:[[:space:]]*//p' "$1/isoloom.yml" | head -1 | tr -d '"'"'"; }
LAB_NAME=$(lab_name "$CYBERCTF_TEST_LOCAL_LAB")
# The id a local import gets (labs::local_id): `local-<slug of the name>`.
LAB_ID="local-$(printf '%s' "$LAB_NAME" | tr 'A-Z' 'a-z' | sed 's/[^a-z0-9_-]/-/g; s/^-*//; s/-*$//' | cut -c1-56)"
case "$OS" in
  Darwin) LABS_DIR="$HOME/Library/Application Support/org.cyberctf.desktop/labs" ;;
  *) LABS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/org.cyberctf.desktop/labs" ;;
esac

# --- Running the specs ------------------------------------------------------------------------
# A spec's check lines (support.mjs `results`) are its summary on screen, as the integration
# runner shows its tests' checks.
# shellcheck disable=SC2034 # read by logged (scripts/lib/suite.sh)
SUITE_SHOW='^(ok|FAILED|skipped):'
stale_apps() { pkill -f "$APP_BIN" 2>/dev/null && sleep 1; }

# check_rows <spec> <log>: a summary line per check the spec printed (`<spec>/<check>`), which
# the summary's readers (the test rig) take as they take a spec's; not counted again.
check_rows() {
  local spec=$1 log=$2 line status id secs note
  while IFS= read -r line; do
    status=${line%%:*}
    line=${line#*: }
    id=${line%% *}
    line=${line#* (}
    secs=${line%%s)*}
    note=${line#*s) }
    [ "$id" = spec ] && continue
    case $status in ok) ;; FAILED) status=FAIL ;; skipped) status=skip ;; esac
    SUITE_RESULTS+=("$(printf '%-20s %-5s %6ss  %s' "$spec/$id" "$status" "$secs" "$note")")
  done < <(grep -E '^(ok|FAILED|skipped): [^ ]+ \([0-9]+s\) ' "$log")
}

# run_logged <name> <timeout> <command...>: one spec, alone (no app left from before), its output
# in its log, killed past its timeout. Exit 0 passes, 2 skips (its `skipped: spec` line says why),
# anything else fails; then a summary line per check it printed.
run_logged() {
  local name=$1 limit=$2 start=$SECONDS log rc why
  shift 2
  wanted "$name" || return 0
  stale_apps
  log="$LOGS/$name.log"
  logged "$name" "$log" "$limit" "$@"
  rc=$?
  stale_apps
  case $rc in
    0) record "$name" ok $((SECONDS - start)) "$log" ;;
    2)
      why=$(sed -n 's/^skipped: spec (0s) //p' "$log" | head -n 1)
      record "$name" skip $((SECONDS - start)) "${why:-skipped (see $log)}"
      ;;
    *) record "$name" FAIL $((SECONDS - start)) "$log (exit $rc)" ;;
  esac
  check_rows "$name" "$log"
}
# run_spec <name> <spec file> <timeout> [VAR=value...]: e2e/desktop/specs/<file>.mjs with those knobs.
run_spec() {
  local name=$1 file=$2 limit=$3
  shift 3
  run_logged "$name" "$limit" env "$@" node "e2e/desktop/specs/$file.mjs"
}
spec() { run_spec "$1" "$1" "$CASE_TIMEOUT"; }

# UTM on a Mac: a started VM named `<lab> · <machine>` is what the running-labs scan looks for, where
# `<lab>` is the installed lab's VM-name prefix: its isoloom name plus `-<instance>` when the lab holds
# an instance number (`.cyberctf-instance`, 1 and up), the plain name otherwise. An empty QEMU VM
# is enough, it boots to its firmware and shows as started.
UTM_VM_ID=""
UTMCTL=/Applications/UTM.app/Contents/MacOS/utmctl
drop_utm_vm() {
  if [ -n "$UTM_VM_ID" ]; then
    "$UTMCTL" stop --kill "$UTM_VM_ID" >/dev/null 2>&1
    sleep 2
    "$UTMCTL" delete "$UTM_VM_ID" >/dev/null 2>&1
    UTM_VM_ID=""
  fi
}
vm_scan() {
  wanted vm-scan || return 0
  if [ "$OS" != Darwin ]; then
    skip vm-scan "the UTM fixture is macOS only; elsewhere start a libvirt/Hyper-V VM named for the lab and run e2e/desktop/specs/vm-scan.mjs with CYBERCTF_EXPECT_LAB"
    return
  fi
  [ -x "$UTMCTL" ] || { skip vm-scan "UTM is not installed"; return; }
  [ -d "$LABS_DIR/$LAB_ID" ] || { skip vm-scan "the lab is not installed under $LABS_DIR (the launch spec imports it)"; return; }
  local arch vm prefix instance
  arch=$(uname -m | sed 's/^arm64$/aarch64/')
  prefix="$LAB_NAME"
  instance=$(tr -d '[:space:]' <"$LABS_DIR/$LAB_ID/.cyberctf-instance" 2>/dev/null)
  case "$instance" in
    '' | *[!0-9]* | 0) ;;
    *) prefix="$LAB_NAME-$instance" ;;
  esac
  vm="$prefix · dc01"
  say "UTM fixture VM: $vm"
  UTM_VM_ID=$(osascript \
    -e 'tell application "UTM"' \
    -e "set vm to make new virtual machine with properties {backend:qemu, configuration:{name:\"$vm\", architecture:\"$arch\", memory:256, cpu cores:1}}" \
    -e 'start vm' \
    -e 'return id of vm' \
    -e 'end tell' 2>"$LOGS/utm-fixture.log") || { skip vm-scan "could not create the UTM fixture VM ($(tail -1 "$LOGS/utm-fixture.log"))"; return; }
  for _ in $(seq 1 15); do
    "$UTMCTL" status "$UTM_VM_ID" 2>/dev/null | grep -q started && break
    sleep 2
  done
  # The script makes Docker unreachable for the app, so only the hypervisor probe can report the lab.
  run_spec vm-scan vm-scan "$CASE_TIMEOUT" CYBERCTF_EXPECT_LAB="$LAB_ID"
  drop_utm_vm
}

# What a run leaves behind: the imported lab's containers (its Compose project and attack box).
# `cleanup_lab final` is the run's last word and honours KEEP; between specs it always runs.
cleanup_lab() {
  [ "${1:-}" = final ] && [ -n "${KEEP:-}" ] && return 0
  [ "$DOCKER_OK" -eq 1 ] || return 0
  if docker compose ls -q 2>/dev/null | grep -qx "cyberctf-$LAB_ID"; then
    say "cleanup: docker compose -p cyberctf-$LAB_ID down"
    docker compose -p "cyberctf-$LAB_ID" down --remove-orphans >/dev/null 2>&1
  fi
  # One id per line: each removed (the ids passed as one word failed with more than one), and
  # the next scenario never starts on what is left.
  local leftovers
  leftovers=$(docker ps -aq --filter "name=$LAB_ID")
  [ -z "$leftovers" ] || printf '%s\n' "$leftovers" | xargs docker rm -f >/dev/null
  leftovers=$(docker ps -aq --filter "name=$LAB_ID")
  [ -z "$leftovers" ] || fail_now "containers of $LAB_ID are left after the clean-up: $(echo "$leftovers" | tr '\n' ' ')"
}

on_exit() {
  stale_apps
  drop_utm_vm
  stop_frontend
  restore_session
}
trap on_exit EXIT INT TERM

say "app: $APP_BIN"
say "lab: $CYBERCTF_TEST_LOCAL_LAB ($LAB_NAME, id $LAB_ID)"
[ "$DOCKER_OK" -eq 1 ] || say "docker is not answering: the specs that run a lab here are skipped"

# The embedded WebDriver server answers, or nothing below can: an app built without `wdio`.
preflight() {
  local pid up=0
  TAURI_WEBDRIVER_PORT=$TAURI_WEBDRIVER_PORT "$APP_BIN" >"$LOGS/preflight.log" 2>&1 &
  pid=$!
  for _ in $(seq 1 60); do
    curl -fsS -o /dev/null "http://127.0.0.1:$TAURI_WEBDRIVER_PORT/status" && { up=1; break; }
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  stale_apps
  [ "$up" -eq 1 ] || { tail -15 "$LOGS/preflight.log"; fail_now "no embedded WebDriver server on $TAURI_WEBDRIVER_PORT: the app must be built with --features wdio"; }
}
preflight

# Order: what needs neither Docker nor a lab, then the local lab's specs (each from nothing of the
# lab up), the catalogue's, the VM forms, the VM scan, the server, and the cleanup last (it
# removes the downloads the others use).
# The smoke launches the app itself (smoke.sh), as a player opens it.
run_logged smoke "$CASE_TIMEOUT" e2e/desktop/smoke.sh
for s in title-bar settings onboarding setup-screens machine; do spec "$s"; done
cleanup_lab
spec labs
spec deep-link
spec fail-offline
spec fail-badlab
SIGNED_OUT_HOME=$(mktemp -d "${TMPDIR:-/tmp}/cyberctf-signedout.XXXXXX")
run_spec fail-signedout fail-signedout "$CASE_TIMEOUT" HOME="$SIGNED_OUT_HOME"
rm -rf "$SIGNED_OUT_HOME"
DOCKER_SPECS="fail-docker launch restart stop launch-ui kill-mid-deploy attack-box attack-shell stop-with-box park-resume reimport-running attack-on-stopped lab-actions offline-start form-container"
if [ "$DOCKER_OK" -eq 1 ]; then
  spec fail-docker
  # Nothing of the lab up before launch, so a pass proves a real start and not a leftover;
  # launch, restart and stop go together (the lab the first starts, the last stops).
  cleanup_lab
  spec launch
  spec restart
  spec stop
  for s in launch-ui kill-mid-deploy attack-box attack-shell stop-with-box park-resume reimport-running attack-on-stopped lab-actions offline-start; do
    cleanup_lab
    spec "$s"
  done
  run_spec form-container lab-form "$CASE_TIMEOUT" CYBERCTF_TEST_FORM=container
else
  for s in $DOCKER_SPECS; do skip "$s" "docker is not running"; done
fi
run_spec form-docker-vm lab-form "$VM_TIMEOUT" CYBERCTF_TEST_FORM=docker-vm
run_spec form-vm lab-form "$VM_TIMEOUT" CYBERCTF_TEST_FORM=vm
run_spec form-per-machine lab-form "$VM_TIMEOUT" CYBERCTF_TEST_FORM=per-machine
run_spec screens screens "$VM_TIMEOUT"
vm_scan
run_spec server-lab server-lab "$VM_TIMEOUT"
cleanup_lab final
spec cleanup

suite_end "desktop e2e, logs in $LOGS"
