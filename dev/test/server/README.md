# Test servers

Throwaway Proxmox and ESXi hosts, plus scripts that run a real lab on them the way the
launcher does. Use them to test the launcher's **Server** flow (and a lab's `deploy/`
layer) on any computer. Nothing here is shipped.

Machine-specific things (generated root passwords, the test SSH key, lab checkouts,
Terraform state, a scratch Vagrant home) go to `.state/`, which git ignores. Passwords are
generated on first use into `.state/secrets.env` and never printed.

| Path | What it gives you |
| --- | --- |
| `proxmox-fusion/` | **Proxmox VE 9 on VMware Fusion with nested KVM** (recommended: lab VMs run at full speed) |
| `proxmox/` | Proxmox VE 8 on VirtualBox (Debian 12 + Proxmox repo), `https://192.168.56.10:8006` (no nested KVM on Intel Macs) |
| `esxi/` | ESXi 8.0U3e (the free edition) on VMware Fusion, unattended install |
| `test/local-vm.sh` | a lab on a local VM through `deploy/vagrant` |
| `test/proxmox.sh` | the launcher's Proxmox path: `deploy/terraform/proxmox` in the Terraform container |
| `test/proxmox-range.sh` | a multi-VM range: SDN network + router + 2 VMs via `deploy/terraform/proxmox-range` |
| `test/esxi.sh` | the launcher's ESXi path: `deploy/vagrant` with `vmware_esxi` |

Each test deploys the Supplier Portal API lab (`CyberCTF/invoice-portal-api`, set
`LAB_DIR=<lab checkout>` for another one), waits for it, then does what "Open shell"
does: SSH to the lab host and `docker exec` into the attack box, which must see `web` and
`database`. Pass `KEEP=1` to leave the lab running; otherwise it is destroyed.

## Prerequisites

- Docker (Docker Desktop or Colima): Terraform and the ISO repack run in containers.
- Vagrant 2.4+ (`test/esxi.sh` installs `vagrant-vmware-esxi` into `.state/vagrant-home`,
  not into your own Vagrant).
- Proxmox host: VirtualBox 7. ESXi host: VMware Fusion Pro (free, Broadcom login), which
  also provides `vmrun`, `vmware-vdiskmanager` and `ovftool`.
- Disk: about 30 GB for Proxmox, 80 GB for ESXi. RAM: 8 GB (Proxmox), 12 GB (ESXi).

## Proxmox on Fusion (recommended)

```sh
curl -fLo .state/iso/proxmox-ve_9.2-1.iso https://enterprise.proxmox.com/iso/proxmox-ve_9.2-1.iso
proxmox-fusion/build.sh       # unattended install, ~10 min; prints the IP
test/proxmox.sh               # uses that host automatically (storage local-lvm, bridge vmbr1)
```

`build.sh` prepares the official ISO with Proxmox's `proxmox-auto-install-assistant` (in a
Debian 13 container): an answer file (root password from `.state/secrets.env`, DHCP, ext4
on the NVMe disk) and `first-boot.sh`, which switches apt to the free no-subscription repo,
adds `snippets`/`iso` content to `local`, and creates `vmbr1`, a NAT bridge with DHCP
(192.168.200.0/24) for lab VMs. Lab VMs stay inside the Proxmox VM, so Fusion needs no
promiscuous mode. The VM has 4 vCPU, 16 GB RAM and nested VT-x (`/dev/kvm` works).

In the launcher: Server, Add host, Proxmox, the IP, `root@pam`, node `pve`, storage
`local-lvm`, bridge `vmbr1`, "Self-signed certificate" on.

## Proxmox on VirtualBox

```sh
proxmox/up.sh
test/proxmox.sh              # or NO_KVM=1 test/proxmox.sh, see below
test/proxmox.sh destroy      # if you used KEEP=1
```

In the launcher: Server, Add host, Proxmox, host `192.168.56.10`, user `root@pam`,
password from `.state/secrets.env`, node `pve`, storage `local`, bridge `vmbr0`,
"Self-signed certificate" on.

**Nested virtualization.** Lab VMs inside Proxmox need hardware virtualization passed
through. VirtualBox on an Intel Mac doesn't pass it through ("Processor supports nested
HW virtualization: no"), so there use `NO_KVM=1`: the lab VM starts in software emulation
with CPU model `x86-64-v2-AES` (plain `qemu64` lacks x86-64-v2, which `mysql:8.0` needs).
It works but takes about 25 minutes. On Linux/Windows hosts with nested VT-x/AMD-V, or with
Proxmox on Fusion, no flag is needed.

The test host is Proxmox installed on Debian (the official "Install Proxmox VE on Debian
12" path) with what a stock install has: root SSH with the password (Terraform uploads the
cloud-init snippet over SSH), `iso` + `snippets` content on `local`, and a `vmbr0` bridge
(here NATed, with DHCP from dnsmasq on 10.10.10.0/24). Lab VMs on that bridge aren't
reachable from your computer, so `test/proxmox.sh` runs the SSH check from the node.

## ESXi

1. Download the free ESXi: support.broadcom.com, My Downloads, then **"Free Software
   Downloads available HERE"**, search **VMware vSphere Hypervisor**, 8.0U3e (build
   24677879), tick the terms, download the ISO (it's not in the paid product list).
2. Build and test:

```sh
esxi/build.sh ~/Downloads/VMware-VMvisor-Installer-8.0U3e-24677879.x86_64.iso
test/esxi.sh                 # uses the IP build.sh recorded in .state/esxi-ip
```

`build.sh` repacks the ISO (into `esxi/.vm/`) with a kickstart: install to the
first disk, DHCP, SSH on, and the boot option `systemMediaSize=min`. Without it ESXi 8
gives its OSData partition up to 128 GB and leaves no room for `datastore1` ("No
datastores found on target"). The VM has nested virtualization on (`vhv.enable`), and the
`.vmx` needs the standard PCIe bridge entries or Fusion can't attach the disk controller.

**Nested networking.** Lab VMs on ESXi have their own MAC addresses, so two things must
let them through: ESXi's `vSwitch0` (the kickstart allows promiscuous mode and forged
transmits) and Fusion itself, which on macOS needs an admin's approval to put the ESXi VM's
network card in promiscuous ("network monitoring") mode. Without it lab VMs get no IPv4
address (Vagrant times out waiting for "running"). If you dismissed Fusion's prompt,
disconnect and reconnect the VM's network adapter in Fusion to be asked again. ESXi's own
tools may not report its IP to Fusion; `build.sh` then reads it from Fusion's NAT DHCP leases.

In the launcher: Server, Add host, VMware ESXi, the IP, user `root`, password from
`.state/secrets.env`, SSH port 22.

## The launcher's own code paths

Opt-in Rust tests (from `src-tauri/`):

```sh
# Proxmox apply/status/destroy through the launcher's Terraform runner
CYBERCTF_TEST_DEPLOY=<lab>/deploy CYBERCTF_TEST_PVE_HOST=192.168.56.10 \
CYBERCTF_TEST_PVE_PASSWORD=... CYBERCTF_TEST_PVE_NO_START=1 CYBERCTF_TEST_LAB_COMMIT=<sha> \
  cargo test --lib proxmox_apply_status_destroy -- --ignored --nocapture

# docker/vagrant found with the PATH a Finder-launched app gets
cargo test --lib finder_path_finds_tools -- --ignored --test-threads=1
```

## Cleanup

```sh
(cd proxmox && vagrant destroy -f)
"/Applications/VMware Fusion.app/Contents/Library/vmrun" stop esxi/.vm/cyberctf-esxi-test.vmwarevm/cyberctf-esxi-test.vmx hard
rm -rf esxi/.vm .state
```


## Multi-VM ranges (proxmox-range)

`test/proxmox-range.sh` applies `deploy/terraform/proxmox-range` (SDN zone + a VNet/subnet
per VLAN, a Debian router doing NAT/DHCP/DNS/firewall, and lab VMs as linked clones) with a
minimal attacker(vlan99)+target(vlan10) range, then checks the router, NAT and reachability.
It creates a Debian template (VMID 9000) on the host once. `destroy` or `KEEP=1` as usual.

On the Fusion test host, pass `PVE_UPLINK=vmbr1` so the router's WAN uses the Proxmox host's
internal NAT bridge (the host masquerades it out). Fusion's own NAT won't give nested guest
MACs a lease without promiscuous mode (a macOS admin approval); `vmbr1` sidesteps that. On a
real Proxmox with a real LAN, the default `vmbr0` uplink is correct.

**SDN teardown quirk:** `pvesh delete` of a zone/vnet can fail via Proxmox's staged SDN
config. Reliable recovery: remove the `ctf<N>` blocks from `/etc/pve/sdn/{zones,vnets,subnets}.cfg`
then `pvesh set /cluster/sdn`.
