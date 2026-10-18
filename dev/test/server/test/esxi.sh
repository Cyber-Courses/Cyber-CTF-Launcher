#!/bin/sh
# The launcher's ESXi path: the lab's deploy/vagrant with provider vmware_esxi and the
# CYBERCTF_ESXI_* env the launcher passes, then "Open shell" via vagrant ssh-config.
#   test/esxi.sh [esxi-ip]      # default: the IP esxi/build.sh recorded; KEEP=1 to keep
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
secret ESXI_ROOT_PASSWORD
HOST=${1:-$(cat "$STATE/esxi-ip")}
# Plugin + box in a scratch Vagrant home: the machine's own Vagrant setup is untouched.
export VAGRANT_HOME="$STATE/vagrant-home"
vagrant plugin list | grep -q vagrant-vmware-esxi || vagrant plugin install vagrant-vmware-esxi
export CYBERCTF_ESXI_HOSTNAME="$HOST" CYBERCTF_ESXI_HOSTPORT=22 CYBERCTF_ESXI_USERNAME=root CYBERCTF_ESXI_PASSWORD="$ESXI_ROOT_PASSWORD"
export CYBERCTF_ATTACKBOX_IMAGE=debian:12-slim
LAB=$(lab_dir)
RUN="$STATE/runs/esxi"; rm -rf "$RUN"; mkdir -p "$RUN"
cp -R "$LAB/docker-compose.yml" "$LAB/build" "$LAB/evidence" "$LAB/deploy" "$RUN/"
cd "$RUN/deploy/vagrant"
vagrant up --provider vmware_esxi
CFG=$(vagrant ssh-config)
H=$(echo "$CFG" | awk '/HostName/{print $2}'); U=$(echo "$CFG" | awk '/ User /{print $2}')
K=$(echo "$CFG" | awk '/IdentityFile/{print $2; exit}' | tr -d '"')
echo "== Open shell (the launcher's ssh command, non-interactive probe) -> $U@$H"
ssh -i "$K" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$STATE/known_hosts" -o LogLevel=ERROR "$U@$H" \
  "sudo docker exec attacker sh -c 'echo SHELL-OK \$(hostname); getent hosts web database'"
[ -n "${KEEP:-}" ] || vagrant destroy -f
