#!/usr/bin/env bash
# The leftover audit (tests/leftovers.md): what a run of the launcher's stack left on a machine.
# The rig, a runner or a person takes a snapshot before and after, then diffs them:
#
#   tests/audit/run.sh snapshot [--out <file>] [--root <dir>]... [--sudo]   this machine (Linux, macOS)
#   tests/audit/run.sh diff <before.json> <after.json> [options]            what the run left
#   tests/audit/run.sh inventory <snapshot.json>                            a snapshot's items
#   tests/audit/run.sh --list                                               the kinds it compares
#
# On Windows the snapshot is snapshot.ps1; on a Proxmox or ESXi server, snapshot-server.sh, run
# there. The diff runs wherever Node does (main.ts --help lists its options, among them
# `--results <file>`: a record per kind, suite `audit`, tests/README.md's "Results"). Exit status:
# 0 nothing left, 1 the run left something, 2 the audit couldn't run.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
case "${1:-}" in
  snapshot)
    shift
    exec bash "$here/snapshot.sh" "$@"
    ;;
  -h | --help)
    sed -n '2,/^[^#]/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
    ;;
  diff | inventory | --list)
    exec node --experimental-strip-types --no-warnings=ExperimentalWarning "$here/main.ts" "$@"
    ;;
  *)
    echo "usage: tests/audit/run.sh snapshot|diff|inventory|--list (--help says more)" >&2
    exit 2
    ;;
esac
