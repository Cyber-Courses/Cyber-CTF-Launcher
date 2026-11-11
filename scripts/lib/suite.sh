#!/usr/bin/env bash
# Shared by the repo's suite runners (scripts/release/release-check.sh,
# scripts/check/other-platforms.sh, tests/integration/run.sh, tests/desktop/run.sh): which steps
# run (ONLY / SKIP), one verdict line per step, a step run in the open or logged with a time limit,
# Node from .nvmrc, --help from the runner's header, and the summary that sets the exit status.
# Sourced, never run:
#
#   . "$(dirname "$0")/lib/suite.sh"
#   suite_help "$0" "$@"
#   step lint pnpm lint
#   suite_end "release check"
#
# A runner sets nothing first; the knobs are the caller's environment:
#   ONLY="a b"   run only these steps        SKIP="a b"   run all but these
#   STRICT=1     a skipped step fails the run
#
# A runner run as one step of another (`nested`) exits SUITE_SKIPPED_STATUS when it skipped any of
# its own steps, so the outer runner records that step as skipped, not passed.

# Guarded: sourcing twice keeps the results so far.
if [ -z "${SUITE_LOADED:-}" ]; then
  SUITE_LOADED=1
  SUITE_RESULTS=()
  SUITE_FAILED=0
  SUITE_SKIPPED=0
fi

# The exit status of a nested runner that passed but skipped some of its steps.
SUITE_SKIPPED_STATUS=3

# GUI-installed tools are not always on a shell's PATH (Homebrew, cargo, Docker Desktop).
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.cargo/bin:/Applications/Docker.app/Contents/Resources/bin"

# suite_help <script> <args...>: prints the script's header comment and exits on -h / --help.
suite_help() {
  local script=$1
  shift
  case "${1:-}" in
    -h | --help)
      sed -n '2,/^[^#]/p' "$script" | sed '$d' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
  esac
}

# listed <word> <space-separated list>: whether the list names the word.
listed() { [[ " $2 " == *" $1 "* ]]; }

# wanted <step>: whether ONLY / SKIP let it run.
wanted() {
  if [ -n "${ONLY:-}" ] && ! listed "$1" "$ONLY"; then return 1; fi
  if [ -n "${SKIP:-}" ] && listed "$1" "$SKIP"; then return 1; fi
  return 0
}

# serves <port>: whether a web server answers on this machine's port (a dev server to reuse).
serves() { curl -fsS -o /dev/null "http://127.0.0.1:$1/"; }

# record <step> <ok|FAIL|skip> <seconds> <detail>: one line of the summary.
record() {
  SUITE_RESULTS+=("$(printf '%-20s %-5s %6ss  %s' "$1" "$2" "$3" "$4")")
  case $2 in
    FAIL) SUITE_FAILED=$((SUITE_FAILED + 1)) ;;
    skip) SUITE_SKIPPED=$((SUITE_SKIPPED + 1)) ;;
  esac
}

say() { printf '\n==== %s\n' "$*"; }

# skip <step> <reason>
skip() {
  wanted "$1" || return 0
  say "$1: skipped ($2)"
  record "$1" skip 0 "$2"
}

# step <step> <command...>: runs it in the open (its output is the detail) and records the verdict.
step() {
  local name=$1 start=$SECONDS rc
  shift
  wanted "$name" || return 0
  say "$name: $*"
  "$@"
  rc=$?
  if [ "$rc" -eq 0 ]; then record "$name" ok $((SECONDS - start)) ""; else record "$name" FAIL $((SECONDS - start)) "exit $rc"; fi
}

# nested <step> <runner...>: another suite runner, run as one step of this one. It gets none of
# this runner's ONLY / SKIP (they name this runner's steps, not its own) and its verdict is ok when
# it ran everything, skip when it skipped some of its steps (see suite_end), FAIL otherwise.
nested() {
  local name=$1 start=$SECONDS rc
  shift
  wanted "$name" || return 0
  say "$name: $*"
  ONLY="" SKIP="" SUITE_NESTED=1 "$@"
  rc=$?
  case $rc in
    0) record "$name" ok $((SECONDS - start)) "" ;;
    "$SUITE_SKIPPED_STATUS") record "$name" skip $((SECONDS - start)) "it skipped some of its steps (see its summary above)" ;;
    *) record "$name" FAIL $((SECONDS - start)) "exit $rc" ;;
  esac
}

# logged <step> <log> <timeout seconds, 0: none> <command...>: runs it with its output in <log>,
# killed past the timeout; prints the log's last lines (or what `SUITE_SHOW` greps from it), and
# returns its exit status. The caller records the verdict (an exit status may mean a skip).
logged() {
  local name=$1 log=$2 limit=$3 pid waited=0 rc
  shift 3
  say "$name"
  "$@" >"$log" 2>&1 &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$limit" -gt 0 ] && [ "$waited" -ge "$limit" ]; then
      echo "suite: $name exceeded its ${limit}s" >>"$log"
      kill "$pid" 2>/dev/null
      break
    fi
    sleep 2
    waited=$((waited + 2))
  done
  wait "$pid"
  rc=$?
  if [ -n "${SUITE_SHOW:-}" ]; then grep -E "$SUITE_SHOW" "$log" | sed 's/^/   /'; else tail -n 4 "$log" | sed 's/^/   /'; fi
  return "$rc"
}

# use_nvmrc_node <repo>: the Node major .nvmrc names, through nvm when the shell's is another one.
use_nvmrc_node() {
  local want nvm_sh
  want=$(tr -d '[:space:]' <"$1/.nvmrc")
  node --version 2>/dev/null | grep -q "^v${want}\." && return 0
  nvm_sh="${NVM_DIR:-$HOME/.nvm}/nvm.sh"
  if [ -s "$nvm_sh" ]; then
    set +u
    # shellcheck source=/dev/null
    . "$nvm_sh"
    nvm use "$want" >/dev/null 2>&1 || echo "warning: Node $want is not installed in nvm (nvm install $want)"
    set -u
  else
    echo "warning: using node $(node --version 2>/dev/null || echo none); .nvmrc asks for $want"
  fi
}

# suite_end <title>: the summary, and the exit status: 1 on a failure, or on a skip under STRICT;
# SUITE_SKIPPED_STATUS on a skip when run by another runner (`nested`); 0 otherwise.
suite_end() {
  say "$1"
  # bash 3.2 (macOS) reads an empty array as unset under `set -u`.
  [ "${#SUITE_RESULTS[@]}" -eq 0 ] || printf '   %s\n' "${SUITE_RESULTS[@]}"
  printf '\n%d failed, %d skipped\n' "$SUITE_FAILED" "$SUITE_SKIPPED"
  [ "$SUITE_FAILED" -eq 0 ] || exit 1
  if [ "$SUITE_SKIPPED" -gt 0 ]; then
    [ -z "${STRICT:-}" ] || exit 1
    [ -z "${SUITE_NESTED:-}" ] || exit "$SUITE_SKIPPED_STATUS"
  fi
  exit 0
}
