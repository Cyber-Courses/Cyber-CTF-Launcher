#!/usr/bin/env bash
# The hooks' test: a throwaway repository with these hooks on (core.hooksPath) and a bare remote.
# A clean commit and push go through; an attribution line is refused at commit time and, when
# the commit skipped its hooks, at push time; a staged file over 5 MB is refused unless Git LFS
# holds it; a fake private key is refused at push time; a missing gitleaks refuses the push. The
# repository's own checks (repo.sh) are stubbed to a log: they are CI's gates, run as CI runs them.
#
#   .githooks/test.sh          needs git and gitleaks
set -euo pipefail

hooks=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
log="$tmp/log"
failed=0

ok() { printf 'ok    %s\n' "$1"; }
fail() {
  printf 'FAIL  %s\n' "$1"
  failed=1
}
# in_repo <git args>: git in the throwaway repository, its output kept for `said`.
in_repo() { git -C "$tmp/repo" "$@" >"$tmp/out" 2>&1; }
said() { grep -q -- "$1" "$tmp/out"; }

git init -q --bare "$tmp/origin"
git init -q -b main "$tmp/repo"
mkdir "$tmp/repo/.githooks"
cp "$hooks"/lib.sh "$hooks"/commit-msg "$hooks"/pre-commit "$hooks"/pre-push "$tmp/repo/.githooks/"
# The repository's own checks, stubbed: what was asked goes to the log.
cat >"$tmp/repo/.githooks/repo.sh" <<EOF
format_staged() { printf 'format_staged: %s\n' "\$1" | tr '\n' ' ' >>"$log"; echo >>"$log"; }
gate() { printf 'gate: %s\n' "\$1" | tr '\n' ' ' >>"$log"; echo >>"$log"; }
EOF
in_repo config core.hooksPath .githooks
in_repo config user.name tester
in_repo config user.email tester@example.com
in_repo remote add origin "$tmp/origin"
: >"$log"

# A clean commit, with the staged files handed to the formatter.
echo one >"$tmp/repo/a.txt"
in_repo add a.txt
if in_repo commit -q -m "A clean commit" && grep -q '^format_staged: a.txt $' "$log"; then ok "a clean commit, its staged files formatted"; else
  cat "$tmp/out"
  fail "a clean commit"
fi

# Each attribution line is refused at commit time, whatever its case.
for line in "Co-Authored-By: Someone <s@example.com>" "Claude-Session: https://example.com/session" "generated with an assistant" "🤖 made this"; do
  echo "$line" >>"$tmp/repo/a.txt"
  in_repo add a.txt
  what="commit with '$line'"
  if ! in_repo commit -q -m "A commit" -m "$line" && said "attribution line"; then ok "$what refused"; else
    cat "$tmp/out"
    fail "$what not refused"
  fi
  in_repo reset -q a.txt
  in_repo checkout -q a.txt
done
# A comment line in the message (git strips it) doesn't count.
echo two >>"$tmp/repo/a.txt"
in_repo add a.txt
if in_repo commit -q -m "Another clean commit" -m "# Co-Authored-By: a comment, not a trailer"; then ok "a message's comment line is no attribution"; else
  cat "$tmp/out"
  fail "a comment line was refused"
fi

# A file over 5 MB is refused, unless Git LFS holds it.
dd if=/dev/zero of="$tmp/repo/big.bin" bs=1048576 count=6 2>/dev/null
in_repo add big.bin
what="a 6 MB file"
if ! in_repo commit -q -m "A big file" && said "over 5 MB"; then ok "$what refused"; else
  cat "$tmp/out"
  fail "$what not refused"
fi
echo 'big.bin filter=lfs diff=lfs merge=lfs -text' >"$tmp/repo/.gitattributes"
# Without git-lfs the file is staged as it is (the hook reads the attribute); with it, as a pointer.
in_repo -c filter.lfs.required=false add .gitattributes big.bin
if in_repo commit -q -m "A big file under LFS"; then ok "$what under Git LFS committed"; else
  cat "$tmp/out"
  fail "$what under Git LFS was refused"
fi

# The first push (a new branch) goes through, and runs the gate with the files the commits touch.
: >"$log"
if in_repo push -q origin main && grep -q '^gate: .gitattributes a.txt big.bin $' "$log"; then ok "a clean push, the gate run over the pushed files"; else
  cat "$tmp/out" "$log"
  fail "a clean push"
fi

# A fake private key (gitleaks' private-key rule) is refused at push time, redacted. The text is
# assembled here so this file itself never holds the pattern.
head="-----BEGIN RSA PRIVATE"
printf '%s KEY-----\nNOTAREALKEYNOTAREALKEYNOTAREALKEYNOTAREALKEY\n-----END RSA PRIVATE KEY-----\n' "$head" >"$tmp/repo/key.pem"
in_repo add key.pem
in_repo commit -q -m "A key, by mistake"
what="a push with a private key"
if ! in_repo push -q origin main && said "gitleaks finds a secret" && ! said "NOTAREALKEY"; then ok "$what refused, redacted"; else
  cat "$tmp/out"
  fail "$what not refused, or the key was printed"
fi
in_repo reset -q --hard HEAD~1

# An attribution line that skipped the commit hook (--no-verify) is refused at push time.
echo three >>"$tmp/repo/a.txt"
in_repo add a.txt
in_repo commit -q --no-verify -m "Skipped the hook" -m "Co-Authored-By: Someone <s@example.com>"
what="a push of a commit with an attribution line"
if ! in_repo push -q origin main && said "attribution line"; then ok "$what refused"; else
  cat "$tmp/out"
  fail "$what not refused"
fi
in_repo reset -q --hard HEAD~1

# Without gitleaks the push is refused, with the install line, not scanned less.
echo four >>"$tmp/repo/a.txt"
in_repo add a.txt
in_repo commit -q -m "A later commit"
what="a push without gitleaks"
if ! HOOKS_GITLEAKS="$tmp/no-such-gitleaks" in_repo push -q origin main && said "brew install gitleaks"; then ok "$what refused"; else
  cat "$tmp/out"
  fail "$what not refused"
fi

# A later push (the branch exists on the remote) scans only the new commits and goes through.
: >"$log"
if in_repo push -q origin main && grep -q '^gate: a.txt $' "$log"; then ok "a later clean push, the gate run over its files"; else
  cat "$tmp/out" "$log"
  fail "a later clean push"
fi

exit "$failed"
