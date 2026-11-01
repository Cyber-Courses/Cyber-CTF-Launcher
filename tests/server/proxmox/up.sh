#!/bin/sh
# Builds the throwaway Proxmox VE test host on VirtualBox (see ../README.md).
#   ./up.sh            # all stages; prints how to add it in the launcher
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
cd "$SERVER_TEST/proxmox"
secret PVE_ROOT_PASSWORD
export PVE_ROOT_PASSWORD
vagrant up --provider virtualbox
vagrant reload
vagrant provision --provision-with pve
vagrant provision --provision-with net
until curl -sk -o /dev/null --max-time 5 https://192.168.56.10:8006/; do sleep 5; done
echo "Proxmox is up: https://192.168.56.10:8006  user root@pam, node pve, storage local, bridge vmbr0"
echo "Password: PVE_ROOT_PASSWORD in $STATE/secrets.env"
