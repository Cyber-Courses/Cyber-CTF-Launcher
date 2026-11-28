#!/bin/sh
# Tauri's beforeBundleCommand on macOS (src-tauri/tauri.macos.conf.json). A universal build
# (`tauri build --target universal-apple-darwin`) merges only the app's own binary into
# target/universal-apple-darwin, but the bundle also copies the command line beside it
# (src/bin/cyberctf.rs), so this merges that one too, from the two builds Tauri just made.
# Any other build has nothing to merge.
set -eu
[ "${TAURI_ENV_TARGET_TRIPLE:-}" = universal-apple-darwin ] || exit 0
profile=release
[ "${TAURI_ENV_DEBUG:-false}" = true ] && profile=debug
target="${CARGO_TARGET_DIR:-src-tauri/target}"
lipo -create -output "$target/universal-apple-darwin/$profile/cyberctf" \
  "$target/aarch64-apple-darwin/$profile/cyberctf" "$target/x86_64-apple-darwin/$profile/cyberctf"
echo "universal-cli: merged $target/universal-apple-darwin/$profile/cyberctf"
