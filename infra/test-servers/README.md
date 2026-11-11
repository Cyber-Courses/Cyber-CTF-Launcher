# Test servers

Throwaway Proxmox and ESXi hosts to test the launcher's **Server** flow against, on any
computer. Nothing here is shipped. Once a host is up, the integration tests run a lab on it the
way the launcher does: `CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh` with the host's knobs
(`docs/testing/integration.md`).

Machine-specific things (generated root passwords, the test SSH key, lab checkouts,
Terraform state, a scratch Vagrant home) go to `.state/`, which git ignores. Passwords are
generated on first use into `.state/secrets.env` and never printed.

| Path              | What it gives you                                                                                                 |
| ----------------- | ----------------------------------------------------------------------------------------------------------------- |
| `proxmox-fusion/` | **Proxmox VE 9 on VMware Fusion with nested KVM** (recommended: lab VMs run at full speed)                        |
| `proxmox/`        | Proxmox VE 8 on VirtualBox (Debian 12 + Proxmox repo), `https://192.168.56.10:8006` (no nested KVM on Intel Macs) |
| `esxi/`           | ESXi 8.0U3e (the free edition) on VMware Fusion, unattended install                                               |

## Prerequisites

- Docker (Docker Desktop or Colima): Terraform and the ISO repack run in containers.
- Vagrant 2.4+ with the `vagrant-vmware-esxi` plugin, for a lab on ESXi.
- Proxmox host: VirtualBox 7. ESXi host: VMware Fusion Pro (free, Broadcom login), which
  also provides `vmrun`, `vmware-vdiskmanager` and `ovftool`.
- Disk: about 30 GB for Proxmox, 80 GB for ESXi. RAM: 8 GB (Proxmox), 12 GB (ESXi).

## Proxmox on Fusion (recommended)

```sh
curl -fLo .state/iso/proxmox-ve_9.2-1.iso https://enterprise.proxmox.com/iso/proxmox-ve_9.2-1.iso
proxmox-fusion/build.sh       # unattended install, ~10 min; prints the IP
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
```

In the launcher: Server, Add host, Proxmox, host `192.168.56.10`, user `root@pam`,
password from `.state/secrets.env`, node `pve`, storage `local`, bridge `vmbr0`,
"Self-signed certificate" on.

**Nested virtualization.** Lab VMs inside Proxmox need hardware virtualization passed
through. VirtualBox on an Intel Mac doesn't pass it through ("Processor supports nested
HW virtualization: no"): lab VMs then run in software emulation, about 25 minutes for a lab.
On Linux/Windows hosts with nested VT-x/AMD-V, or with Proxmox on Fusion, they run at full
speed.

The test host is Proxmox installed on Debian (the official "Install Proxmox VE on Debian
12" path) with what a stock install has: root SSH with the password (Terraform uploads the
cloud-init snippet over SSH), `iso` + `snippets` content on `local`, and a `vmbr0` bridge
(here NATed, with DHCP from dnsmasq on 10.10.10.0/24). Lab VMs on that bridge aren't
reachable from your computer: the runner reaches them through the node (`CYBERCTF_TEST_PVE_JUMP`).

## ESXi

1. Download the free ESXi: support.broadcom.com, My Downloads, then **"Free Software
   Downloads available HERE"**, search **VMware vSphere Hypervisor**, 8.0U3e (build
   24677879), tick the terms, download the ISO (it's not in the paid product list).
2. Build it:

```sh
esxi/build.sh ~/Downloads/VMware-VMvisor-Installer-8.0U3e-24677879.x86_64.iso   # records its IP in .state/esxi-ip
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

## Running a lab on a host

The integration tests' server tier runs a lab on it through the launcher's own runner path
(`docs/testing/integration.md`), with the host's knobs:

```sh
# Proxmox: the API endpoint and token, the node, the lab bridge; the jump when that bridge is
# internal to the node; a runner home whose ssh/id_ed25519 the node lets in.
CYBERCTF_TEST_PVE_ENDPOINT=https://<ip>:8006/ CYBERCTF_TEST_PVE_TOKEN='root@pam!name=secret' \
CYBERCTF_TEST_PVE_NODE=pve CYBERCTF_TEST_PVE_BRIDGE=vmbr1 CYBERCTF_TEST_PVE_JUMP=root@<ip> \
CYBERCTF_TEST_PVE_RUNNER_HOME=<folder> CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh

# ESXi: the host, its root password, the datastore and port group.
CYBERCTF_TEST_ESXI_HOST=<ip> CYBERCTF_TEST_ESXI_PASSWORD=... CYBERCTF_TEST_ESXI_DATASTORE=datastore1 \
CYBERCTF_TEST_ESXI_NETWORK='VM Network' CYBERCTF_TEST_INTEGRATION_TIERS=server tests/integration/run.sh
```

The launcher's "Test connection" against a Proxmox host has its own opt-in test (from
`src-tauri/`): `CYBERCTF_TEST_PVE_HOST=<ip> CYBERCTF_TEST_PVE_PASSWORD=... cargo test --lib
proxmox_live_test -- --ignored --nocapture`.

## Cleanup

```sh
(cd proxmox && vagrant destroy -f)
"/Applications/VMware Fusion.app/Contents/Library/vmrun" stop esxi/.vm/cyberctf-esxi-test.vmwarevm/cyberctf-esxi-test.vmx hard
rm -rf esxi/.vm .state
```
