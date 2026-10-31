#!/usr/bin/env bash
# Launch the (wdio-feature) app so its embedded WebDriver server listens, then run the smoke.
# macOS/Linux. The app must be built with `--features wdio`.
set -u
HERE="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${APP_BIN:-$HERE/src-tauri/target/debug/cyberctf-desktop}"
[ -x "$BIN" ] || { echo "app binary not found/executable: $BIN"; exit 2; }
export TAURI_WEBDRIVER_PORT="${TAURI_WEBDRIVER_PORT:-4445}"

"$BIN" >/tmp/cyberctf-app.out 2>&1 &
APP=$!
cleanup() { kill "$APP" 2>/dev/null; }
trap cleanup EXIT

# Wait for the embedded WebDriver server's /status endpoint.
up=0
for _ in $(seq 1 40); do
  if curl -fsS "http://127.0.0.1:${TAURI_WEBDRIVER_PORT}/status" >/dev/null 2>&1; then up=1; break; fi
  sleep 1
done
[ "$up" = 1 ] || { echo "embedded WebDriver server never came up on ${TAURI_WEBDRIVER_PORT}"; echo "--- app out ---"; tail -15 /tmp/cyberctf-app.out; exit 3; }

node "$HERE/e2e-native/smoke-embedded.mjs"
rc=$?
echo "--- app out tail ---"; tail -6 /tmp/cyberctf-app.out
exit $rc
