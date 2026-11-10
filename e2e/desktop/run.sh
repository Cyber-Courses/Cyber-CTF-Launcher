#!/usr/bin/env bash
# The native GUI e2e layer, end to end, on this machine (macOS or Linux): build the app with its
# embedded WebDriver server, serve the frontend a debug build loads, prepare the fixtures (a local
# lab to import; on a Mac, a UTM VM named for it), run every native scenario in an order that
# leaves nothing behind, and print one verdict per scenario. This is the layer CI does not run
# (it needs a desktop session, a real Docker engine and, for the VM scan, a hypervisor), so it is
# part of scripts/release-check.sh, the gate before a release.
#
#   e2e/desktop/run.sh                          # everything this machine can run
#   ONLY="smoke fail-offline" e2e/desktop/run.sh
#   SKIP_BUILD=1 KEEP=1 e2e/desktop/run.sh      # reuse the binary; leave the lab running
#
# Knobs:
#   APP_BIN                 the app to drive (default: the wdio debug build, built here); a
#                           release build needs NEXT_PUBLIC_WDIO=1 at its build for the __wdio_*
#                           hooks the scenarios use (docs/testing/desktop-e2e.md)
#   TAURI_WEBDRIVER_PORT    the embedded WebDriver port (4445)
#   DEV_URL_PORT            where a DEBUG build loads its frontend from (3206, tauri.conf.json's
#                           devUrl); a `next dev` already listening there is reused
#   CYBERCTF_TEST_LOCAL_LAB the lab folder the scenarios import (default: a checkout of the
#                           Supplier Portal API lab under infra/test-servers/.state/labs)
#   CASE_TIMEOUT            seconds per scenario (900: the first import builds the lab's images)
#   ONLY, SKIP, STRICT      as every suite runner (scripts/lib/suite.sh)
# Each scenario is e2e/desktop/specs/<name>.mjs; docs/testing/desktop-e2e.md says what each proves.
# Windows: e2e/desktop/smoke.ps1 runs the smoke; the scenarios take APP_BIN there too.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=../../scripts/lib/suite.sh
. "$HERE/scripts/lib/suite.sh"
suite_help "$0" "$@"
cd "$HERE" || exit 2
NATIVE_STATE="$HERE/e2e/desktop/.state"
LOGS="$NATIVE_STATE/logs"
mkdir -p "$LOGS"

CASE_TIMEOUT=${CASE_TIMEOUT:-900}
DEV_URL_PORT=${DEV_URL_PORT:-3206}
export TAURI_WEBDRIVER_PORT=${TAURI_WEBDRIVER_PORT:-4445}
OS=$(uname -s)

fail_now() { printf 'run.sh: %s\n' "$*" >&2; exit 2; }

use_nvmrc_node "$HERE"
command -v pnpm >/dev/null || fail_now "pnpm is not installed"

# --- The app, built with the embedded WebDriver server --------------------------------------
APP_BIN=${APP_BIN:-$HERE/src-tauri/target/debug/cyberctf-desktop}
export APP_BIN
if [ -z "${SKIP_BUILD:-}" ]; then
  say "cargo build --features wdio"
  cargo build --manifest-path src-tauri/Cargo.toml --features wdio || fail_now "the wdio build failed"
fi
[ -x "$APP_BIN" ] || fail_now "app binary not found or not executable: $APP_BIN"
[ -d node_modules/webdriverio ] || pnpm install --frozen-lockfile || fail_now "pnpm install failed"

# --- The frontend a debug build loads (tauri.conf.json devUrl); a release build embeds it ----
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
      pnpm exec next dev -p "$DEV_URL_PORT" > "$LOGS/next-dev.log" 2>&1 &
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

# --- Running the scenarios --------------------------------------------------------------------
stale_apps() { pkill -f "$APP_BIN" 2>/dev/null && sleep 1; }
# run_case <name> <command...>: alone (no app left from before), its output in its log, killed
# past CASE_TIMEOUT; its exit status is the verdict.
run_case() {
  local name=$1 start=$SECONDS log rc
  shift
  wanted "$name" || return 0
  stale_apps
  log="$LOGS/$name.log"
  logged "$name" "$log" "$CASE_TIMEOUT" "$@"
  rc=$?
  stale_apps
  if [ "$rc" -eq 0 ]; then record "$name" ok $((SECONDS - start)) "$log"; else record "$name" FAIL $((SECONDS - start)) "$log (exit $rc)"; fi
}
run_mjs() { run_case "$1" node "e2e/desktop/specs/$1.mjs"; }

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
  [ -d "$LABS_DIR/$LAB_ID" ] || { skip vm-scan "the lab is not installed under $LABS_DIR (the launch scenario imports it)"; return; }
  local arch vm prefix instance
  arch=$(uname -m | sed 's/^arm64$/aarch64/')
  prefix="$LAB_NAME"
  instance=$(tr -d '[:space:]' < "$LABS_DIR/$LAB_ID/.cyberctf-instance" 2>/dev/null)
  case "$instance" in
    ''|*[!0-9]*|0) ;;
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
  CYBERCTF_EXPECT_LAB="$LAB_ID" run_case vm-scan node e2e/desktop/specs/vm-scan.mjs
  drop_utm_vm
}

# What a run leaves behind: the imported lab's containers (its Compose project and attack box).
# `cleanup_lab final` is the run's last word and honours KEEP; between scenarios it always runs.
cleanup_lab() {
  [ "${1:-}" = final ] && [ -n "${KEEP:-}" ] && return 0
  [ "$DOCKER_OK" -eq 1 ] || return 0
  if docker compose ls -q 2>/dev/null | grep -qx "cyberctf-$LAB_ID"; then
    say "cleanup: docker compose -p cyberctf-$LAB_ID down"
    docker compose -p "cyberctf-$LAB_ID" down --remove-orphans >/dev/null 2>&1
  fi
  local leftovers
  leftovers=$(docker ps -q --filter "name=$LAB_ID" 2>/dev/null)
  [ -n "$leftovers" ] && docker rm -f "$leftovers" >/dev/null 2>&1
  return 0
}

on_exit() {
  stale_apps
  drop_utm_vm
  stop_frontend
}
trap on_exit EXIT INT TERM

say "app: $APP_BIN"
say "lab: $CYBERCTF_TEST_LOCAL_LAB ($LAB_NAME, id $LAB_ID)"
[ "$DOCKER_OK" -eq 1 ] || say "docker is not answering: the scenarios that run a lab are skipped"

# Order: the ones that need nothing, then the Docker ones, then the VM scan (the import left the
# lab installed), then cleanup. A scenario that imports the lab itself (the launch hook, the attack
# box) or must catch its deploy under way starts from a stopped lab: importing a running one is
# refused, which is a different scenario.
run_case smoke e2e/desktop/smoke.sh
run_mjs fail-offline
run_mjs fail-badlab
SIGNED_OUT_HOME=$(mktemp -d "${TMPDIR:-/tmp}/cyberctf-signedout.XXXXXX")
HOME="$SIGNED_OUT_HOME" run_case fail-signedout node e2e/desktop/specs/fail-signedout.mjs
rm -rf "$SIGNED_OUT_HOME"
# Setup/UI scenarios that need neither Docker nor a lab.
run_mjs onboarding
run_mjs setup-screens
if [ "$DOCKER_OK" -eq 1 ]; then
  run_mjs fail-docker
  # Nothing of the lab up before launch, so a pass proves a real start and not a leftover.
  cleanup_lab
  run_mjs launch
  run_mjs restart
  run_mjs stop
  cleanup_lab
  run_mjs launch-ui
  cleanup_lab
  run_mjs kill-mid-deploy
  cleanup_lab
  run_mjs attack-box
  cleanup_lab
  run_mjs stop-with-box
  cleanup_lab
  run_mjs park-resume
  cleanup_lab
  run_mjs reimport-running
  cleanup_lab
  run_mjs attack-on-stopped
else
  for c in fail-docker launch restart stop launch-ui attack-box kill-mid-deploy stop-with-box park-resume reimport-running attack-on-stopped; do skip "$c" "docker is not running"; done
fi
vm_scan
# A catalogue container lab in a VM on UTM through the real "Where should it run?" dialog (needs
# the app signed in, UTM and its Vagrant plugin): the docker-in-VM path end to end, from the UI.
vm_launch() {
  wanted launch-vm || return 0
  [ "$OS" = Darwin ] || { skip launch-vm "the VM launch runs on UTM (macOS); elsewhere run e2e/desktop/specs/launch-vm.mjs with CYBERCTF_TEST_VM_PROVIDER"; return; }
  [ -x "$UTMCTL" ] || { skip launch-vm "UTM is not installed"; return; }
  command -v vagrant >/dev/null && vagrant plugin list 2>/dev/null | grep -q '^vagrant_utm ' || { skip launch-vm "Vagrant with the vagrant_utm plugin is not installed"; return; }
  # Not through run_case: the scenario skips itself (exit 2) when the app isn't signed in or the
  # lab isn't in this account's catalogue, and it bounds its own wait (15 min for the VM to come up).
  local log="$LOGS/launch-vm.log" start=$SECONDS rc
  stale_apps
  logged launch-vm "$log" 0 node e2e/desktop/specs/launch-vm.mjs
  rc=$?
  case $rc in
    0) record launch-vm ok $((SECONDS - start)) "$log" ;;
    2) record launch-vm skip $((SECONDS - start)) "$(grep -m1 '^LAUNCH_VM_SKIP' "$log" | cut -c16-)" ;;
    *) record launch-vm FAIL $((SECONDS - start)) "$log (exit $rc)" ;;
  esac
}
vm_launch
cleanup_lab final

suite_end "native e2e, logs in $LOGS"
