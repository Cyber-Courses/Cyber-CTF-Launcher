#!/bin/sh
# A lab on a local VM (deploy/vagrant, VirtualBox by default), then the attack box check.
#   test/local-vm.sh            # KEEP=1 to leave the VM running; PROVIDER=vmware_desktop etc.
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
LAB=$(lab_dir)
RUN="$STATE/runs/local-vm"; rm -rf "$RUN"; mkdir -p "$RUN"
cp -R "$LAB/docker-compose.yml" "$LAB/build" "$LAB/evidence" "$LAB/deploy" "$RUN/"
cd "$RUN/deploy/vagrant"
CYBERCTF_ATTACKBOX_IMAGE=debian:12-slim vagrant up --provider "${PROVIDER:-virtualbox}"
vagrant ssh -c "sudo docker compose -p lab -f /opt/lab/docker-compose.yml ps -a; sudo docker exec attacker sh -c 'echo SHELL-OK \$(hostname); getent hosts web database'"
[ -n "${KEEP:-}" ] || vagrant destroy -f
