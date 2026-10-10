# shellcheck shell=sh
# Shared helpers for the test servers (sourced, POSIX sh).
# Everything machine-specific (passwords, keys, lab checkouts, Terraform state) lives in
# dev/test/server/.state/, which git ignores.

# Callers set SERVER_TEST (this folder) before sourcing:
#   SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
STATE="$SERVER_TEST/.state"
mkdir -p "$STATE"

# GUI-installed tools are not always on PATH.
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin:/Applications/VMware Fusion.app/Contents/Library:/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool"

# shellcheck disable=SC2034 # used by the scripts that source this file
TERRAFORM_IMAGE=hashicorp/terraform:1.16.5

# secret NAME: load NAME from .state/secrets.env, generating it on first use (never printed).
secret() {
  f="$STATE/secrets.env"
  touch "$f" && chmod 600 "$f"
  if ! grep -q "^$1=" "$f"; then
    printf "%s='Cx%s9!'\n" "$1" "$(openssl rand -base64 24 | tr -dc 'A-Za-z0-9' | head -c 16)" >> "$f"
  fi
  # shellcheck source=/dev/null
  . "$f"
}

# lab_dir: a lab to deploy (LAB_DIR, else a checkout of the Supplier Portal API lab).
lab_dir() {
  if [ -n "${LAB_DIR:-}" ]; then echo "$LAB_DIR"; return; fi
  d="$STATE/labs/invoice-portal-api"
  if [ -d "$d/.git" ]; then git -C "$d" pull -q; else git clone -q https://github.com/CyberCTF/invoice-portal-api "$d"; fi
  echo "$d"
}

# test_key: the stand-in for the launcher's SSH key (.state/test_key).
test_key() {
  [ -f "$STATE/test_key" ] || ssh-keygen -q -t ed25519 -N "" -C cyberctf-launcher-test -f "$STATE/test_key"
  echo "$STATE/test_key"
}
