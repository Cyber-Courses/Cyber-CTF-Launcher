#!/bin/sh
# The launcher's Proxmox path: the lab's deploy/terraform/proxmox in the Terraform
# container, cloud-init + Ansible in the VM, then "Open shell" over SSH with a test key.
#   test/proxmox.sh             # PVE_HOST (default: the Fusion host if built, else 192.168.56.10), KEEP=1
#   PVE_STORAGE / PVE_BRIDGE    # default local-lvm + vmbr1 on the Fusion host, local + vmbr0 on VirtualBox
#   test/proxmox.sh destroy
# PVE_AUTH=token: sign in with an API token (root@pam!cyberctf-test, privilege separation
# off) and upload the snippet over SSH with the test key, like a launcher token host.
# NO_KVM=1 for a Proxmox host without nested virtualization (the VirtualBox test host on
# an Intel Mac): creates the VM stopped, then starts it in software emulation (slow).
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
secret PVE_ROOT_PASSWORD
if [ -z "${PVE_HOST:-}" ] && [ -f "$STATE/pve-fusion-ip" ]; then
  HOST=$(cat "$STATE/pve-fusion-ip"); : "${PVE_STORAGE:=local-lvm}" "${PVE_BRIDGE:=vmbr1}"
else
  HOST=${PVE_HOST:-192.168.56.10}
fi
# Commands on the Proxmox node: SSH as root with the generated password (no sshpass needed).
ASKPASS="$STATE/askpass-pve"
printf '#!/bin/sh\necho "$PVE_ROOT_PASSWORD"\n' > "$ASKPASS"; chmod 700 "$ASKPASS"; export PVE_ROOT_PASSWORD
node() { SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force DISPLAY=x ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$STATE/known_hosts" -o PreferredAuthentications=password,keyboard-interactive -o LogLevel=ERROR "root@$HOST" "$@"; }
LAB=$(lab_dir)
RUN="$STATE/runs/proxmox"; mkdir -p "$RUN"
rm -rf "$RUN/deploy"; cp -R "$LAB/deploy" "$RUN/"
[ -z "${NO_KVM:-}" ] || printf 'resource "proxmox_virtual_environment_vm" "labhost" {\n  started = false\n}\n' > "$RUN/deploy/terraform/proxmox/test_override.tf"
KEY=$(test_key)
AUTH="-e TF_VAR_proxmox_password=$PVE_ROOT_PASSWORD"
if [ "${PVE_AUTH:-password}" = token ]; then
  # A fresh token each run (the secret is only shown at creation), and the test key on root.
  SECRET=$(node "pveum user token remove root@pam cyberctf-test >/dev/null 2>&1; pveum user token add root@pam cyberctf-test --privsep 0 --output-format json" | sed -n 's/.*"value":"\([^"]*\)".*/\1/p')
  [ -n "$SECRET" ] || { echo "couldn't create the API token"; exit 1; }
  node "grep -qF '$(cut -d' ' -f2 "$KEY.pub")' ~/.ssh/authorized_keys || echo '$(cat "$KEY.pub")' >> ~/.ssh/authorized_keys"
  cp "$KEY" "$RUN/ssh_key"; chmod 600 "$RUN/ssh_key"
  AUTH="-e TF_VAR_proxmox_api_token=root@pam!cyberctf-test=$SECRET -e TF_VAR_proxmox_ssh_private_key_file=/state/ssh_key"
  echo "== API token root@pam!cyberctf-test (SSH with the test key)"
fi
tf() {
  docker run --rm --entrypoint sh -v "$RUN/deploy:/deploy" -v "$RUN:/state" -w /deploy/terraform/proxmox \
    -e TF_DATA_DIR=/state/.terraform \
    -e TF_VAR_proxmox_endpoint="https://$HOST:8006/" -e TF_VAR_proxmox_username=root@pam $AUTH \
    -e TF_VAR_proxmox_insecure=true -e TF_VAR_proxmox_ssh_address="$HOST" -e TF_VAR_proxmox_storage="${PVE_STORAGE:-local}" -e TF_VAR_proxmox_bridge="${PVE_BRIDGE:-vmbr0}" \
    -e TF_VAR_cpu_type="${PVE_CPU:-$([ -n "${NO_KVM:-}" ] && echo x86-64-v2-AES || echo host)}" \
    -e TF_VAR_lab_slug=invoice-portal-api -e TF_VAR_lab_repository=CyberCTF/invoice-portal-api -e TF_VAR_lab_commit="$(git -C "$LAB" rev-parse HEAD)" \
    -e TF_VAR_attackbox_image=debian:12-slim -e TF_VAR_ssh_public_key="$(cat "$KEY.pub")" \
    "$TERRAFORM_IMAGE" -c "terraform init -input=false -no-color -backend-config=path=/state/terraform.tfstate >/dev/null && terraform $1 -auto-approve -input=false -no-color"
}
if [ "${1:-apply}" = destroy ]; then tf destroy; exit; fi
tf apply
VMID=$(docker run --rm -v "$RUN:/state" --entrypoint sh "$TERRAFORM_IMAGE" -c "grep -o '\"vm_id\": *[0-9][0-9]*' /state/terraform.tfstate | head -1 | grep -o '[0-9]*$'")
if [ -n "${NO_KVM:-}" ]; then node "qm set $VMID --kvm 0 >/dev/null && qm start $VMID"; fi
echo "Waiting for cloud-init + Ansible in VM $VMID..."
# The bootstrap's status file: "running: <step>", "ready" or "failed: <step>" (older labs
# only touch /var/lib/cyberctf-lab-ready).
last=""
while :; do
  out=$(node "qm guest exec $VMID -- sh -c 'cat /var/lib/cyberctf/status 2>/dev/null || { test -f /var/lib/cyberctf-lab-ready && echo ready; }'" 2>/dev/null | grep -o '"out-data" : "[^"]*' | sed 's/.*: "//; s/\\n$//') || true
  [ "$out" = "$last" ] || { [ -z "$out" ] || echo "   lab host: $out"; last=$out; }
  case "$out" in
    ready) break ;;
    failed:*) node "qm guest exec $VMID -- tail -n 40 /var/log/cyberctf-lab.log" | sed 's/\\n/\n/g'; [ -n "${KEEP:-}" ] || tf destroy; exit 1 ;;
  esac
  sleep 20
done
IP=$(node "qm guest cmd $VMID network-get-interfaces" 2>/dev/null | grep -o '"ip-address" : "[0-9.]*' | grep -o '[0-9.]*$' | grep -v '^127\.' | grep -v '^172\.1[7-9]\.' | head -1)
echo "== Open shell (the launcher's ssh command) from the node -> debian@$IP"
# From the node (lab VMs sit on its bridge), with the test key copied over for the check.
node "cat > /tmp/cyberctf-testkey && chmod 600 /tmp/cyberctf-testkey" < "$KEY"
node "ssh -i /tmp/cyberctf-testkey -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/tmp/kh -o LogLevel=ERROR debian@$IP \"sudo docker exec attacker sh -c 'echo SHELL-OK \\\$(hostname); getent hosts web database'\"; rm -f /tmp/cyberctf-testkey /tmp/kh"
[ -n "${KEEP:-}" ] || tf destroy
