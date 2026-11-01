#!/usr/bin/env bash
# The native GUI e2e layer, end to end, on this machine (macOS or Linux): build the app with its
# embedded WebDriver server, serve the frontend a debug build loads, prepare the fixtures (a local
# lab to import; on a Mac, a UTM VM named for it), run every native scenario in an order that
# leaves nothing behind, and print one verdict per scenario. This is the layer CI does not run
# (it needs a desktop session, a real Docker engine and, for the VM scan, a hypervisor), so it is
# part of scripts/release-check.sh, the gate before a release.
#
#   tests/native/run-all.sh                          # everything this machine can run
#   ONLY="smoke fail-offline" tests/native/run-all.sh
#   SKIP_BUILD=1 KEEP=1 tests/native/run-all.sh      # reuse the binary; leave the lab running
#
# Knobs:
#   APP_BIN                 the app to drive (default: the wdio debug build, built here)
#   TAURI_WEBDRIVER_PORT    the embedded WebDriver port (4445)
#   DEV_URL_PORT            where a DEBUG build loads its frontend from (3000, tauri.conf.json's
#                           devUrl); a `next dev` already listening there is reused
#   CYBERCTF_TEST_LOCAL_LAB the lab folder the scenarios import (default: a checkout of the
#                           Supplier Portal API lab under tests/server/.state/labs)
#   CASE_TIMEOUT            seconds per scenario (900: the first import builds the lab's images)
#   STRICT=1                a skipped scenario fails the run
# Windows: tests/native/run-embedded.ps1 runs the smoke; the scenarios take APP_BIN there too.
set -uo pipefail

HERE=$(cd "$(dirname "$0")/../.." && pwd)
cd "$HERE" || exit 2
NATIVE_STATE="$HERE/tests/native/.state"
LOGS="$NATIVE_STATE/logs"
mkdir -p "$LOGS"
# GUI-installed tools are not always on a shell's PATH.
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin"

CASE_TIMEOUT=${CASE_TIMEOUT:-900}
DEV_URL_PORT=${DEV_URL_PORT:-3000}
export TAURI_WEBDRIVER_PORT=${TAURI_WEBDRIVER_PORT:-4445}
OS=$(uname -s)

say() { printf '\n== %s\n' "$*"; }
fail_now() { printf 'run-all: %s\n' "$*" >&2; exit 2; }

# --- Node: the version .nvmrc names, through nvm when the shell's node is another major -------
want=$(tr -d '[:space:]' < .nvmrc)
if ! node --version 2>/dev/null | grep -q "^v${want}\."; then
  nvm_sh="${NVM_DIR:-$HOME/.nvm}/nvm.sh"
  if [ -s "$nvm_sh" ]; then
    set +u
    # shellcheck source=/dev/null
    . "$nvm_sh"
    nvm use "$want" >/dev/null || fail_now "Node $want is not installed in nvm (nvm install $want)"
    set -u
  else
    echo "warning: using node $(node --version 2>/dev/null || echo none); .nvmrc asks for $want"
  fi
fi
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
    if curl -fsS -o /dev/null "http://127.0.0.1:$DEV_URL_PORT/"; then
      say "frontend: reusing the dev server on port $DEV_URL_PORT"
    else
      say "frontend: next dev on port $DEV_URL_PORT (a debug build loads devUrl, not out/)"
      set -m
      pnpm exec next dev -p "$DEV_URL_PORT" > "$LOGS/next-dev.log" 2>&1 &
      NEXT_PGID=$!
      set +m
      for _ in $(seq 1 90); do
        curl -fsS -o /dev/null "http://127.0.0.1:$DEV_URL_PORT/" && break
        sleep 2
      done
      curl -fsS -o /dev/null "http://127.0.0.1:$DEV_URL_PORT/" || { tail -20 "$LOGS/next-dev.log"; stop_frontend; fail_now "next dev never answered on $DEV_URL_PORT"; }
    fi
    ;;
esac

# --- Fixtures ---------------------------------------------------------------------------------
DOCKER_OK=1
docker info >/dev/null 2>&1 || DOCKER_OK=0
if [ -z "${CYBERCTF_TEST_LOCAL_LAB:-}" ]; then
  # The same lab the server tests deploy, checked out under tests/server/.state (ignored).
  SERVER_TEST="$HERE/tests/server"
  # shellcheck source=tests/server/lib.sh
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
RESULTS=()
FAILED=0
SKIPPED=0
wanted() { [ -z "${ONLY:-}" ] || [[ " $ONLY " == *" $1 "* ]]; }
record() { RESULTS+=("$(printf '%-18s %-7s %5ss  %s' "$1" "$2" "$3" "$4")"); }
skip() {
  wanted "$1" || return 0
  say "$1: skipped ($2)"
  SKIPPED=$((SKIPPED + 1))
  record "$1" skip 0 "$2"
}
stale_apps() { pkill -f "$APP_BIN" 2>/dev/null && sleep 1; }
# run_case <name> <command...>: the command's output goes to the log; its exit code is the verdict.
run_case() {
  local name=$1
  shift
  wanted "$name" || return 0
  stale_apps
  local log="$LOGS/$name.log" start=$SECONDS waited=0 pid rc
  say "$name"
  "$@" > "$log" 2>&1 &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$CASE_TIMEOUT" ]; then
      echo "run-all: ${name} exceeded CASE_TIMEOUT=${CASE_TIMEOUT}s" >> "$log"
      kill "$pid" 2>/dev/null
      stale_apps
      break
    fi
    sleep 2
    waited=$((waited + 2))
  done
  wait "$pid"
  rc=$?
  sed 's/^/   /' < <(tail -n 4 "$log")
  if [ "$rc" -eq 0 ]; then
    record "$name" ok $((SECONDS - start)) "$log"
  else
    FAILED=$((FAILED + 1))
    record "$name" FAIL $((SECONDS - start)) "$log (exit $rc)"
  fi
}
run_mjs() { run_case "$1" node "tests/native/functional-$1.mjs"; }

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
    skip vm-scan "the UTM fixture is macOS only; elsewhere start a libvirt/Hyper-V VM named for the lab and run functional-vm-scan.mjs with CYBERCTF_EXPECT_LAB"
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
  CYBERCTF_EXPECT_LAB="$LAB_ID" run_case vm-scan node tests/native/functional-vm-scan.mjs
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
run_case smoke tests/native/run-embedded.sh
run_mjs fail-offline
run_mjs fail-badlab
SIGNED_OUT_HOME=$(mktemp -d "${TMPDIR:-/tmp}/cyberctf-signedout.XXXXXX")
HOME="$SIGNED_OUT_HOME" run_case fail-signedout node tests/native/functional-fail-signedout.mjs
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
cleanup_lab final

say "results"
printf '   %s\n' "${RESULTS[@]}"
printf '\n%d failed, %d skipped, logs in %s\n' "$FAILED" "$SKIPPED" "$LOGS"
if [ "$FAILED" -gt 0 ]; then exit 1; fi
if [ -n "${STRICT:-}" ] && [ "$SKIPPED" -gt 0 ]; then exit 1; fi
exit 0
