# shellcheck shell=bash
# The desktop run's own folder, sourced by run.sh (run.ps1's are lib/suite.ps1's Initialize-RunRoot,
# Copy-RunSession and Clear-RunRoot) and held by its unit test (tests/desktop/run.test.ts). The app
# (a wdio build given it as CYBERCTF_WDIO_ROOT) keeps all of its own there: app/ its data,
# settings, caches, logs and webview data, home/ its session and secrets (src-tauri's
# platform/test_root.rs); records/ is the runner's (lib/owned.mjs). Sourced, never run.

# run_root_make: makes a fresh folder for the run under TMPDIR and prints its real path, as Docker
# records the Compose files' paths (macOS's TMPDIR is under a link).
run_root_make() {
  local dir
  dir=$(mktemp -d "${TMPDIR:-/tmp}/cyberctf-e2e.XXXXXX") || return 1
  (cd "$dir" && pwd -P)
}

# run_root_place_session <root> <session file>: the session where the run's app reads it,
# whatever the build (home/.cyberctf/dev-session.json), this user's alone.
run_root_place_session() {
  mkdir -p "$1/home/.cyberctf" || return 1
  (umask 077 && cp "$2" "$1/home/.cyberctf/dev-session.json")
}

# run_root_drop <root>: removes the run's folder, unless CYBERCTF_TEST_DESKTOP_KEEP is set; says
# where it is when it stays.
run_root_drop() {
  if [ -n "${CYBERCTF_TEST_DESKTOP_KEEP:-}" ]; then
    echo "kept: the run's folder $1 (CYBERCTF_TEST_DESKTOP_KEEP)"
  else
    rm -rf "$1" 2>/dev/null
    [ ! -e "$1" ] || echo "couldn't remove all of the run's folder $1"
  fi
}
