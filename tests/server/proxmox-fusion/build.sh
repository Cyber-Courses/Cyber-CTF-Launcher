#!/bin/sh
# A Proxmox VE 9 test host on VMware Fusion with nested KVM (lab VMs run at full speed).
# Unattended: the official ISO is prepared with proxmox-auto-install-assistant.
#   ./build.sh [proxmox-ve_9.2-1.iso]     # default: .state/iso/proxmox-ve_9.2-1.iso
# In the launcher: Server, Proxmox, the printed IP, root@pam, node pve, storage local-lvm,
# bridge vmbr1, self-signed certificate on.
set -eu
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
ISO=${1:-$STATE/iso/proxmox-ve_9.2-1.iso}
[ -f "$ISO" ] || { echo "Download it: curl -fLo $STATE/iso/proxmox-ve_9.2-1.iso https://enterprise.proxmox.com/iso/proxmox-ve_9.2-1.iso"; exit 1; }
secret PVE_ROOT_PASSWORD
FUSION="/Applications/VMware Fusion.app/Contents/Library"
VMRUN="$FUSION/vmrun"
NAME=cyberctf-pve-test
HERE="$SERVER_TEST/proxmox-fusion"
OUT="$HERE/.vm"
VM="$OUT/$NAME.vmwarevm"
mkdir -p "$OUT" "$VM"

# 1. Answer file + first-boot script into the ISO.
cat > "$OUT/answer.toml" <<TOML
[global]
keyboard = "en-us"
country = "fr"
fqdn = "pve.cyberctf.test"
mailto = "root@localhost"
timezone = "UTC"
root-password = "$PVE_ROOT_PASSWORD"
reboot-mode = "reboot"

[network]
source = "from-dhcp"

[disk-setup]
filesystem = "ext4"
disk-list = ["nvme0n1"]

[first-boot]
source = "from-iso"
ordering = "fully-up"
TOML
chmod 600 "$OUT/answer.toml"
[ -f "$OUT/pve-auto.iso" ] || docker run --rm -v "$(cd "$(dirname "$ISO")" && pwd)/$(basename "$ISO"):/in.iso:ro" -v "$OUT:/out" -v "$HERE/first-boot.sh:/first-boot.sh:ro" debian:13 sh -euc '
  apt-get -qq update >/dev/null && apt-get -qq install -y curl ca-certificates >/dev/null
  curl -fsSL https://enterprise.proxmox.com/debian/proxmox-archive-keyring-trixie.gpg -o /usr/share/keyrings/proxmox-archive-keyring.gpg
  echo "deb [signed-by=/usr/share/keyrings/proxmox-archive-keyring.gpg] http://download.proxmox.com/debian/pve trixie pve-no-subscription" > /etc/apt/sources.list.d/pve.list
  apt-get -qq update >/dev/null && apt-get -qq install -y proxmox-auto-install-assistant xorriso >/dev/null
  proxmox-auto-install-assistant validate-answer /out/answer.toml
  proxmox-auto-install-assistant prepare-iso /in.iso --fetch-from iso --answer-file /out/answer.toml --on-first-boot /first-boot.sh --output /out/pve-auto.iso
'

# 2. The VM: EFI, 4 vCPU / 16 GB, nested VT-x, NVMe 120 GB, vmxnet3 on Fusion NAT.
[ -f "$VM/disk.vmdk" ] || "$FUSION/vmware-vdiskmanager" -c -s 120GB -a lsilogic -t 0 "$VM/disk.vmdk" >/dev/null
cat > "$VM/$NAME.vmx" <<VMX
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "21"
displayName = "$NAME"
guestOS = "debian12-64"
firmware = "efi"
numvcpus = "4"
memsize = "16384"
vhv.enable = "TRUE"
floppy0.present = "FALSE"
pciBridge0.present = "TRUE"
pciBridge4.present = "TRUE"
pciBridge4.virtualDev = "pcieRootPort"
pciBridge4.functions = "8"
pciBridge5.present = "TRUE"
pciBridge5.virtualDev = "pcieRootPort"
pciBridge5.functions = "8"
pciBridge6.present = "TRUE"
pciBridge6.virtualDev = "pcieRootPort"
pciBridge6.functions = "8"
pciBridge7.present = "TRUE"
pciBridge7.virtualDev = "pcieRootPort"
pciBridge7.functions = "8"
nvme0.present = "TRUE"
nvme0:0.present = "TRUE"
nvme0:0.fileName = "disk.vmdk"
sata0.present = "TRUE"
sata0:1.present = "TRUE"
sata0:1.deviceType = "cdrom-image"
sata0:1.fileName = "$OUT/pve-auto.iso"
ethernet0.present = "TRUE"
ethernet0.virtualDev = "vmxnet3"
ethernet0.connectionType = "nat"
ethernet0.addressType = "generated"
VMX

"$VMRUN" -T fusion start "$VM/$NAME.vmx" nogui || { echo "VM failed to start, see $VM/vmware.log"; exit 1; }
echo "Installing Proxmox (unattended, ~10 min)..."
lease_ip() {
  mac=$(grep -i '^ethernet0.generatedAddress ' "$VM/$NAME.vmx" | cut -d'"' -f2)
  [ -n "$mac" ] && grep -B8 -i "hardware ethernet $mac" /var/db/vmware/vmnet-dhcpd-vmnet8.leases 2>/dev/null | awk '/^lease/{ip=$2} END{print ip}'
}
while :; do
  IP=$(lease_ip || true)
  case "$IP" in [0-9]*) curl -sk -o /dev/null --max-time 5 "https://$IP:8006/" && break ;; esac
  sleep 20
done
# First boot (repo, vmbr1, dnsmasq, storage content) runs once the host is fully up.
until ssh -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@"$IP" true 2>/dev/null || [ -n "$(curl -sk --max-time 5 https://$IP:8006/ -o /dev/null -w ok)" ]; do sleep 10; done
echo "$IP" > "$STATE/pve-fusion-ip"
echo "Proxmox is up: https://$IP:8006  root@pam (PVE_ROOT_PASSWORD in $STATE/secrets.env), node pve, storage local-lvm, bridge vmbr1"
