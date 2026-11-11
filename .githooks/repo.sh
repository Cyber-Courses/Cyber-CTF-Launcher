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

# gate: what CI's web and rust jobs refuse first, in a few seconds: Prettier over everything
# format:check covers, rustfmt over the crate.
gate() {
  pnpm format:check || refuse "pnpm format:check fails: pnpm format, commit that, push again"
  cargo fmt --manifest-path src-tauri/Cargo.toml --check || refuse "cargo fmt --check fails: pnpm format, commit that, push again"
}
