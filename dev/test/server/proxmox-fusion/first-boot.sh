#!/bin/bash
# Proxmox test host, first boot (embedded in the ISO by build.sh).
set -eux
# Free repository (no subscription); drop the enterprise ones that need a key.
rm -f /etc/apt/sources.list.d/pve-enterprise.sources /etc/apt/sources.list.d/ceph.sources
cat > /etc/apt/sources.list.d/proxmox.sources <<'SRC'
Types: deb
URIs: http://download.proxmox.com/debian/pve
Suites: trixie
Components: pve-no-subscription
Signed-By: /usr/share/keyrings/proxmox-archive-keyring.gpg
SRC
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y dnsmasq
# vmbr1: a NAT bridge with DHCP for lab VMs (like a home LAN behind the server). Lab VMs
# stay inside the Proxmox VM, so the Mac's hypervisor needs no promiscuous mode.
if ! grep -q vmbr1 /etc/network/interfaces; then
  cat >> /etc/network/interfaces <<'NET'

auto vmbr1
iface vmbr1 inet static
    address 192.168.200.1/24
    bridge-ports none
    bridge-stp off
    bridge-fd 0
    post-up echo 1 > /proc/sys/net/ipv4/ip_forward
    post-up iptables -t nat -A POSTROUTING -s 192.168.200.0/24 -o vmbr0 -j MASQUERADE
    post-down iptables -t nat -D POSTROUTING -s 192.168.200.0/24 -o vmbr0 -j MASQUERADE
NET
fi
cat > /etc/dnsmasq.d/vmbr1.conf <<'DNS'
interface=vmbr1
bind-interfaces
dhcp-range=192.168.200.100,192.168.200.250,12h
dhcp-option=option:router,192.168.200.1
dhcp-option=option:dns-server,1.1.1.1
DNS
ifreload -a
systemctl restart dnsmasq
# Cloud images and cloud-init snippets on "local" (Terraform needs both).
pvesm set local --content iso,vztmpl,backup,snippets,import
