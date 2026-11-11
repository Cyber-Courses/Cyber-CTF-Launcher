#!/usr/bin/env bash
# The checks of the files around the app that CI's lint-infra.yml and scripts/release-check.sh both
# run, written once here so the two can't drift:
#
#   scripts/check-infra.sh pins         every action pinned to a full commit SHA, its version in a comment
#   scripts/check-infra.sh shellcheck   shellcheck on the repository's shell scripts, warnings and errors
#
# Run from anywhere; exits non-zero with what is wrong. Bash 3.2 (macOS) and later.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2

case "${1:-}" in
  pins)
    # A tag or branch can be moved to new code, which then runs with this repo's secrets. Covers
    # the composite actions under .github/actions/ as well as the workflows.
    unpinned=$(grep -nE '^\s*(- )?uses:' .github/workflows/*.yml .github/actions/*/action.yml | grep -vE 'uses: \./' | grep -vE 'uses: [^@[:space:]]+@[0-9a-f]{40} # \S')
    [ -z "$unpinned" ] || {
      echo "$unpinned"
      echo "pin these actions to a full commit SHA, with the version in a comment (uses: owner/repo@<sha> # vX.Y.Z)"
      exit 1
    }
    ;;
  shellcheck)
    # Real scripts only: test fixtures under testdata/ are sample inputs. Read line by line: bash
    # 3.2 has no mapfile. Intentional exceptions are marked in the scripts with a reason.
    scripts=()
    while IFS= read -r f; do scripts+=("$f"); done < <(git ls-files '*.sh' '*.bash' 'images/attack-box/bashrc.cyberctf' ':(exclude)*/testdata/*')
    shellcheck --severity=warning --external-sources "${scripts[@]}"
    ;;
  -h | --help)
    sed -n '2,/^[^#]/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
    ;;
  *)
    echo "usage: scripts/check-infra.sh pins|shellcheck" >&2
    exit 2
    ;;
esac
