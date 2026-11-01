#!/bin/sh
# A throwaway ESXi 8 host in VMware Fusion (nested virtualization on) for testing the
# launcher's ESXi path. Unattended: the ESXi ISO is repacked with a kickstart file.
#
#   ./build.sh /path/to/VMware-VMvisor-Installer-8.0U3e-24677879.x86_64.iso
#
# Needs VMware Fusion (vmrun, vmware-vdiskmanager) and Docker (to repack the ISO).
# Then add a host in the launcher with the printed IP, user root, SSH port 22.
set -eu

ISO=${1:?usage: build.sh <esxi-installer.iso>}
SERVER_TEST=$(cd "$(dirname "$0")/.." && pwd); . "$SERVER_TEST/lib.sh"
# Generated once per machine into .state/secrets.env (or set it yourself).
[ -n "${ESXI_ROOT_PASSWORD:-}" ] || secret ESXI_ROOT_PASSWORD
FUSION="/Applications/VMware Fusion.app/Contents/Library"
VMRUN="$FUSION/vmrun"
NAME=cyberctf-esxi-test
HERE=$(cd "$(dirname "$0")" && pwd)
OUT="$HERE/.vm"
VM="$OUT/$NAME.vmwarevm"
mkdir -p "$OUT" "$VM"

# 1. Kickstart: install to the first disk, DHCP, SSH on (vagrant-vmware-esxi needs it).
#    systemMediaSize=min (boot option below) caps ESXi's OSData partition, otherwise it
#    takes up to 128 GB and leaves no room for datastore1 on the 80 GB disk.
cat > "$OUT/KS.CFG" <<KS
vmaccepteula
install --firstdisk --overwritevmfs
rootpw $ESXI_ROOT_PASSWORD
network --bootproto=dhcp --device=vmnic0
reboot

%firstboot --interpreter=busybox
vim-cmd hostsvc/enable_ssh
vim-cmd hostsvc/start_ssh
vim-cmd hostsvc/enable_esx_shell
esxcli system settings advanced set -o /UserVars/SuppressShellWarning -i 1
# Nested lab VMs have their own MACs: let vSwitch0 pass them (Fusion must also allow the
# ESXi VM into promiscuous mode, see README).
esxcli network vswitch standard policy security set -v vSwitch0 --allow-promiscuous=true --allow-forged-transmits=true --allow-mac-change=true
KS

# 2. Repack the ISO with the kickstart, booting it automatically (BIOS + UEFI).
[ -f "$OUT/esxi-ks.iso" ] || docker run --rm -v "$(cd "$(dirname "$ISO")" && pwd)/$(basename "$ISO"):/in.iso:ro" -v "$OUT:/out" debian:12 sh -euc '
  apt-get -qq update >/dev/null && apt-get -qq install -y xorriso >/dev/null
  mkdir /iso && xorriso -osirrox on -indev /in.iso -extract / /iso >/dev/null 2>&1
  chmod -R u+w /iso
  cp /out/KS.CFG /iso/KS.CFG
  for cfg in /iso/BOOT.CFG /iso/EFI/BOOT/BOOT.CFG; do
    sed -i "s#^kernelopt=.*#kernelopt=runweasel ks=cdrom:/KS.CFG systemMediaSize=min#" "$cfg"
  done
  xorriso -as mkisofs -relaxed-filenames -J -R -o /out/esxi-ks.iso \
    -b ISOLINUX.BIN -c BOOT.CAT -no-emul-boot -boot-load-size 4 -boot-info-table \
    -eltorito-alt-boot -e EFIBOOT.IMG -no-emul-boot /iso >/dev/null 2>&1
'

# 3. The VM: EFI, 4 vCPU / 12 GB, nested VT-x, vmxnet3 on Fusion NAT, 80 GB disk.
[ -f "$VM/disk.vmdk" ] || "$FUSION/vmware-vdiskmanager" -c -s 80GB -a pvscsi -t 0 "$VM/disk.vmdk" >/dev/null
cat > "$VM/$NAME.vmx" <<VMX
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "21"
displayName = "$NAME"
guestOS = "vmkernel8"
firmware = "efi"
numvcpus = "4"
memsize = "12288"
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
scsi0.present = "TRUE"
scsi0.virtualDev = "pvscsi"
scsi0:0.present = "TRUE"
scsi0:0.fileName = "disk.vmdk"
sata0.present = "TRUE"
sata0:1.present = "TRUE"
sata0:1.deviceType = "cdrom-image"
sata0:1.fileName = "$OUT/esxi-ks.iso"
ethernet0.present = "TRUE"
ethernet0.virtualDev = "vmxnet3"
ethernet0.connectionType = "nat"
ethernet0.addressType = "generated"
VMX

"$VMRUN" -T fusion start "$VM/$NAME.vmx" nogui || { echo "VM failed to start, see $VM/vmware.log"; exit 1; }
echo "Installing ESXi (unattended, ~10 min)..."
# ESXi's built-in tools report the IP late or not at all: fall back to Fusion's NAT DHCP
# lease for the VM's MAC address.
lease_ip() {
  mac=$(grep -i '^ethernet0.generatedAddress ' "$VM/$NAME.vmx" | cut -d'"' -f2)
  [ -n "$mac" ] && grep -B8 -i "hardware ethernet $mac" /var/db/vmware/vmnet-dhcpd-vmnet8.leases 2>/dev/null | awk '/^lease/{ip=$2} END{print ip}'
}
guest_ip() { "$VMRUN" -T fusion getGuestIPAddress "$VM/$NAME.vmx" 2>/dev/null || lease_ip || true; }
while :; do
  IP=$(guest_ip)
  case "$IP" in [0-9]*) nc -z -G 3 "$IP" 22 2>/dev/null && break ;; esac
  sleep 20
done
# The installer's own IP answers before the reboot into the installed system; wait for HTTPS too.
until curl -sk -o /dev/null --max-time 5 "https://$IP/"; do sleep 15; done
echo "ESXi is up: https://$IP  user root, SSH on 22"
echo "Password: ESXI_ROOT_PASSWORD in $STATE/secrets.env"
echo "$IP" > "$STATE/esxi-ip"
