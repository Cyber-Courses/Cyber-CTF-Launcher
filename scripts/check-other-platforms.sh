#!/usr/bin/env bash
# Compiles and lints the Rust core for the platforms this machine is not (Linux and Windows on
# x64), so code behind `#[cfg(target_os = …)]` is checked before CI's matrix runs it. Clippy only:
# nothing is linked and no test runs. The C dependencies' build scripts (dbus, aws-lc, ring, the
# Windows resource) need a cross toolchain this machine may not have; stand-ins that answer every
# probe and write empty objects take their place, which is enough for `cargo check`, never for a
# build.
#
#   scripts/check-other-platforms.sh                  every other platform
#   ONLY="linux" scripts/check-other-platforms.sh
#
# Steps: linux (x86_64-unknown-linux-gnu), windows (x86_64-pc-windows-msvc). Needs rustup (a
# Rust from Homebrew or a distribution can't add targets): without it the run stops. A target that
# isn't installed is skipped (rustup target add <triple>).
#
# Knobs:
#   ONLY, SKIP, STRICT  as every suite runner (scripts/lib/suite.sh)
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=lib/suite.sh
. "$ROOT/scripts/lib/suite.sh"
suite_help "$0" "$@"
cd "$ROOT/src-tauri" || exit 2
# The targets rustup has installed; a Rust without rustup has none to list, and stops the run.
INSTALLED=$(rustup target list --installed) || { echo "rustup is required: the other platforms' targets come from it (https://rustup.rs)" >&2; exit 2; }

# Under target/, at a fixed path: the C crates' build scripts rerun when their compiler's path
# changes, so a fixed one keeps their outputs cached between runs.
FAKES="$ROOT/src-tauri/target/stand-in-toolchain"
mkdir -p "$FAKES" || exit 2

# pkg-config: every library is present, at a recent version, with no flags.
cat >"$FAKES/pkg-config" <<'EOF'
#!/bin/sh
for a in "$@"; do case "$a" in --modversion) echo 99.0; exit 0 ;; esac; done
exit 0
EOF
# A C compiler: an empty file wherever the output goes (`-o out`, `-oout`, MSVC's `-Fo`/`/Fo`).
cat >"$FAKES/cc" <<'EOF'
#!/bin/sh
out=""; prev=""
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  case "$a" in -o) ;; -o*) out="${a#-o}" ;; -Fo*) out="${a#-Fo}" ;; /Fo*) out="${a#/Fo}" ;; esac
  prev="$a"
done
[ -n "$out" ] && : >"$out"
exit 0
EOF
# An archiver: GNU-style (`crs lib.a …`) or lib.exe-style (`-out:lib.lib …`).
cat >"$FAKES/ar" <<'EOF'
#!/bin/sh
for a in "$@"; do
  case "$a" in -out:* | /OUT:* | /out:*) : >"${a#*:}"; exit 0 ;; *.a | *.lib) : >"$a"; exit 0 ;; esac
done
exit 0
EOF
# llvm-rc (the Windows resource compiler): says what it is, and writes an empty `/fo` file.
cat >"$FAKES/rc" <<'EOF'
#!/bin/sh
prev=""
for a in "$@"; do
  case "$a" in "/?") echo "OVERVIEW: LLVM Resource Converter /no-preprocess"; exit 0 ;; esac
  [ "$prev" = "/fo" ] && : >"$a"
  prev="$a"
done
exit 0
EOF
chmod +x "$FAKES"/*

# lint <step> <target triple>: clippy on every target of the crate, warnings denied.
lint() {
  local name=$1 triple=$2 var
  wanted "$name" || return 0
  if ! grep -qx "$triple" <<<"$INSTALLED"; then
    skip "$name" "rustup target add $triple"
    return 0
  fi
  var=$(echo "$triple" | tr '-' '_')
  step "$name" env PKG_CONFIG="$FAKES/pkg-config" PKG_CONFIG_ALLOW_CROSS=1 \
    "CC_$var=$FAKES/cc" "CXX_$var=$FAKES/cc" "AR_$var=$FAKES/ar" "RC_$var=$FAKES/rc" \
    cargo clippy --target "$triple" --all-targets -- -D warnings
}

lint linux x86_64-unknown-linux-gnu
lint windows x86_64-pc-windows-msvc
suite_end "other platforms"
