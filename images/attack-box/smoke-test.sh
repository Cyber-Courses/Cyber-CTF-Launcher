#!/usr/bin/env bash
# Smoke test of a built attack box image: started the way the launcher starts it
# (src-tauri/src/runtime/exegol.rs: `docker run -d --init --cap-add NET_ADMIN <image> sleep infinity`),
# then what the launcher and the player rely on from inside it.
# Usage: images/attack-box/smoke-test.sh <image>
set -euo pipefail

image=${1:?usage: smoke-test.sh <image>}
name=attack-box-smoke-$$

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT

fail() {
  echo "::error::$*"
  exit 1
}

docker run -d --init --name "$name" --hostname attacker --cap-add NET_ADMIN "$image" sleep infinity >/dev/null
[ "$(docker inspect -f '{{.State.Running}}' "$name")" = true ] || fail "the box does not stay up"
echo "architecture: $(docker exec "$name" uname -m)"

# The in-app shell is `docker exec -it <box> bash`; closing it runs a POSIX sh loop over /proc
# (end_session_script) with tr, grep and kill.
for tool in bash sh tr grep kill; do
  docker exec "$name" sh -c "command -v $tool" >/dev/null || fail "missing $tool (the launcher's shell needs it)"
done

# The real-labs test reaches a target with curl from inside the box; the rest is the standard
# Kali toolset (kali-linux-headless) players expect.
for tool in curl ip nmap python3 sqlmap hydra john nikto msfconsole; do
  docker exec "$name" sh -c "command -v $tool" >/dev/null || fail "missing $tool"
done

# NET_ADMIN is granted so the player can change routes and interfaces.
docker exec "$name" ip link set lo mtu 65535 || fail "cannot change interfaces with NET_ADMIN"

# The bashrc keeps one stable terminal title (images/attack-box/bashrc.cyberctf).
title=$(docker exec "$name" bash -ic 'printf %s "$PROMPT_COMMAND"' 2>/dev/null)
case "$title" in
  *"Cyber CTF attack box"*) ;;
  *) fail "the shell does not set the Cyber CTF title (PROMPT_COMMAND=$title)" ;;
esac

echo "attack box OK: $image"
