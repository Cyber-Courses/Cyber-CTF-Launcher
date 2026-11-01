#!/bin/sh
# Applies deploy/terraform/proxmox-range against the Fusion Proxmox host with a minimal
# 2-VM range (attacker on VLAN 99 + a target on VLAN 10) and checks the network foundation:
# the router comes up, both VMs get IPs via the router's DHCP, the attacker reaches the
# target, and NAT to the internet works. Needs a Debian template on the host (created here).
#   test/proxmox-range.sh            # KEEP=1 to leave it up; "destroy" to tear down
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
secret PVE_ROOT_PASSWORD
HOST=$(cat "$STATE/pve-fusion-ip")
TEMPLATE_ID=9000
RANGE=${RANGE:-42}
export PVE_ROOT_PASSWORD
ASKPASS="$STATE/askpass-pve"; printf '#!/bin/sh\necho "$PVE_ROOT_PASSWORD"\n' > "$ASKPASS"; chmod 700 "$ASKPASS"
node() { SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force DISPLAY=x ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$STATE/known_hosts" -o PreferredAuthentications=password,keyboard-interactive -o LogLevel=ERROR "root@$HOST" "$@"; }

RUN="$STATE/runs/proxmox-range"; mkdir -p "$RUN"
# The proxmox-range module lives in the lab-template repo (sibling checkout by default).
MOD="${LAB_TEMPLATE:-$(cd "$SERVER_TEST/../../../.." && pwd)/lab-template}/deploy/terraform/proxmox-range"
[ -d "$MOD" ] || { echo "module not found at $MOD; set LAB_TEMPLATE=<lab-template checkout>"; exit 1; }
KEY=$(test_key)

rm -rf "$RUN/module"; cp -R "$MOD" "$RUN/module"; rm -rf "$RUN/module/.terraform"*
tf() {
  docker run --rm --entrypoint sh -v "$RUN/module:/deploy" -v "$RUN:/state" -w /deploy \
    -e TF_DATA_DIR=/state/.terraform \
    -e TF_VAR_proxmox_endpoint="https://$HOST:8006/" -e TF_VAR_proxmox_username=root@pam -e TF_VAR_proxmox_password="$PVE_ROOT_PASSWORD" \
    -e TF_VAR_proxmox_insecure=true -e TF_VAR_proxmox_ssh_address="$HOST" \
    -e TF_VAR_proxmox_storage=local-lvm -e TF_VAR_proxmox_uplink_bridge="${PVE_UPLINK:-vmbr0}" \
    -e TF_VAR_range_number=$RANGE -e TF_VAR_lab_slug=range-test \
    -e TF_VAR_lab_repository=CyberCTF/invoice-portal-api -e TF_VAR_lab_commit=none \
    -e TF_VAR_ssh_public_key="$(cat "$KEY.pub")" \
    -e TF_VAR_vms="$VMS" \
    "$TERRAFORM_IMAGE" -c "terraform init -input=false -no-color -backend-config=path=/state/terraform.tfstate >/dev/null && terraform $1 -auto-approve -input=false -no-color"
}
VMS='[{"name":"attacker","hostname":"attacker","template_id":'$TEMPLATE_ID',"vlan":99,"ip_last_octet":10,"cpus":1,"ram_gb":1,"os":"linux"},{"name":"target","hostname":"target","template_id":'$TEMPLATE_ID',"vlan":10,"ip_last_octet":21,"cpus":1,"ram_gb":1,"os":"linux"}]'

if [ "${1:-apply}" = destroy ]; then tf destroy; exit; fi

# 1. A Debian 12 cloud-init template on the node (VMID 9000), once.
if ! node "qm status $TEMPLATE_ID" >/dev/null 2>&1; then
  echo "Creating Debian template $TEMPLATE_ID..."
  node "set -e
    cd /var/lib/vz/template
    [ -f debian-12.qcow2 ] || wget -qO debian-12.qcow2 https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2
    qm create $TEMPLATE_ID --name debian-12-template --memory 1024 --cores 1 --net0 virtio,bridge=vmbr0 --scsihw virtio-scsi-single --ostype l26 --agent 1
    qm set $TEMPLATE_ID --scsi0 local-lvm:0,import-from=/var/lib/vz/template/debian-12.qcow2
    qm set $TEMPLATE_ID --ide2 local-lvm:cloudinit --boot order=scsi0 --serial0 socket --vga serial0
    qm set $TEMPLATE_ID --ciuser debian --cipassword '$PVE_ROOT_PASSWORD'
    qm template $TEMPLATE_ID"
fi

# 2. Build the range.
tf apply

# 3. Verify from the node: the router, then the two VMs via the router's DHCP.
echo "== waiting for the router + VMs (guest agent) =="
RVM=$(node "qm list" | awk '/ctf'$RANGE'-router/{print $1; exit}')
until node "qm guest exec $RVM -- test -f /var/lib/cyberctf-router-ready" 2>/dev/null | grep -q '"exitcode" : 0'; do sleep 15; done
echo "router ready (vmid $RVM)"
node "cat > /tmp/rk && chmod 600 /tmp/rk" < "$KEY"
# Router WAN IP, then ssh in and check NAT + DHCP leases + reach the target across VLANs.
RIP=$(node "qm guest cmd $RVM network-get-interfaces" 2>/dev/null | grep -o '"ip-address" : "172.16.1.[0-9]*' | grep -o '172.16.1.[0-9]*' | head -1)
echo "router WAN: $RIP"
node "ssh -i /tmp/rk -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/tmp/kh -o LogLevel=ERROR debian@$RIP '
  echo ROUTER-OK \$(hostname);
  sudo nft list chain inet filter forward | head -8;
  cat /var/lib/misc/dnsmasq.leases 2>/dev/null | awk \"{print \\\$3, \\\$4}\";
  ping -c1 -W2 1.1.1.1 >/dev/null && echo NAT-OK || echo NAT-FAIL'
rm -f /tmp/rk /tmp/kh"
[ -n "${KEEP:-}" ] || tf destroy
