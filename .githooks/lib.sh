# shellcheck shell=sh
# What the hooks share (sourced by commit-msg, pre-commit and pre-push, never run). Each hook sets
# HOOK to its name first. repo.sh beside this file holds what this repository checks.

# The lines a commit message may not carry: an assistant's attribution, which the history keeps
# out (CONTRIBUTING, "Git hooks"). Matched case-insensitively, anywhere in the line.
ATTRIBUTION='Co-Authored-By|Claude-Session|Generated with|🤖'

# refuse <why> [<detail>...]: say why the hook refuses, the details indented under it, and stop git.
refuse() {
  printf '%s hook: %s\n' "$HOOK" "$1" >&2
  shift
  for line in "$@"; do printf '  %s\n' "$line" >&2; done
  exit 1
}

# attribution_lines: the lines on stdin that carry an attribution; a message's comment lines (#)
# don't count. Exit status 0 when there are any.
attribution_lines() { grep -v '^#' | grep -E -i "$ATTRIBUTION"; }

# staged_files: the files the commit adds or changes (not the ones it deletes), one a line.
staged_files() { git diff --cached --name-only --diff-filter=ACMR; }
