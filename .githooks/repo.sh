# shellcheck shell=sh
# What this repository checks (lib.sh has what every hook shares): the staged files' formatting
# before a commit, the quickest of CI's gates before a push. Sourced by pre-commit and pre-push;
# refuse (lib.sh) stops git with the reason.

# format_staged <files>: Prettier over the staged files `pnpm format:check` covers (its globs in
# package.json; .prettierignore applies), rustfmt over the staged Rust (src-tauri/rustfmt.toml).
format_staged() {
  web=$(printf '%s\n' "$1" | grep -E '^(src|scripts|tests)/.*\.(ts|tsx|css|mjs)$|^([^/]+|(docs|infra|scripts|tests)/.*)\.md$')
  if [ -n "$web" ]; then
    # shellcheck disable=SC2086 # one file a word: the repository's paths have no spaces
    pnpm exec prettier --check $web || refuse "Prettier would change a staged file: pnpm format, then stage it again"
  fi
  rust=$(printf '%s\n' "$1" | grep -E '\.rs$')
  if [ -n "$rust" ]; then
    # shellcheck disable=SC2086
    rustfmt --check --edition 2024 $rust || refuse "rustfmt would change a staged file: pnpm format, then stage it again"
  fi
}

# gate: what CI's web and rust jobs refuse, in the order they are quick: Prettier over everything
# format:check covers and rustfmt over the crate (seconds), the feature registry, the Tauri
# versions and the docs' links (seconds), then clippy over every target and the quality ratchet
# (clippy, ESLint, duplication, dead code, exceptions: minutes when the build is warm). The slow
# two are what CI refused five pushes in a row on 2026-11-13; HOOKS_QUICK=1 skips them for a
# push that can't wait, which CI then judges.
gate() {
  pnpm format:check || refuse "pnpm format:check fails: pnpm format, commit that, push again"
  cargo fmt --manifest-path src-tauri/Cargo.toml --check || refuse "cargo fmt --check fails: pnpm format, commit that, push again"
  pnpm check:features || refuse "pnpm check:features fails: the feature registry names a test that isn't there (tests/features.yml), or tests/FEATURES.md is stale (pnpm features)"
  pnpm check:tauri || refuse "pnpm check:tauri fails: a Tauri JS plugin and its Rust crate drifted apart"
  pnpm check:docs || refuse "pnpm check:docs fails: a link in the Markdown resolves to nothing"
  if [ "${HOOKS_QUICK:-0}" = 1 ]; then
    printf '%s\n' "pre-push: HOOKS_QUICK=1: clippy and the quality ratchet left to CI"
    return 0
  fi
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets || refuse "cargo clippy --all-targets fails on this OS: fix it, commit that, push again (CI runs it on macOS and Windows too: scripts/check/other-platforms.sh)"
  pnpm quality || refuse "pnpm quality fails: a ratchet went up (clippy or ESLint warnings, a duplicated block, dead code, an allowance without a reason); fix it or, when it is right, raise quality-baseline.json in the same commit"
}
