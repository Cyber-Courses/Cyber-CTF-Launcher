import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { PARSERS, type Item, parseProbe, readSnapshot } from "./inventory.ts";
import { elapsed } from "./parse-host.ts";
import { blocks, epoch, table } from "./parse-util.ts";

// Each parser of the leftover audit against its tool's own output, recorded (fixtures/): on a Mac
// (Docker Desktop, lsof, netstat, VirtualBox 7, UTM), a Linux machine (ss, ip, iptables-save,
// virsh), Windows (snapshot.ps1's JSON lines, netsh), Proxmox VE 9 and ESXi 8.

const fixture = (name: string) => readFileSync(join(import.meta.dirname, "fixtures", name), "utf8");
const parse = (kind: string, file: string, taken = 1_794_312_000): Item[] =>
  parseProbe({ kind, tool: "", status: "ok", note: "", output: fixture(file) }, taken);
const names = (items: Item[]) => items.map((i) => i.name);

describe("Docker's objects", () => {
  it("are told apart by id, named without the leading slash, tied to their Compose project and Isoloom lab", () => {
    const [web, own] = parse("docker-container", "docker-container.txt");
    expect(web).toMatchObject({
      key: "82c307f9f61645f0f063edb60d6fa0e097261dfc9edfdbc5bc351cf05348b4b5",
      name: "cyberctf-hello-web-1",
      created: 1_794_402_325,
    });
    expect(web.refs).toEqual(["cyberctf-hello-web-1", "cyberctf-hello", "hello-stack"]);
    // Where Compose started it from: its working folder and each of its files.
    expect(web.paths).toEqual([
      "/home/player/.local/share/org.cyberctf.desktop/labs/hello/.isoloom/docker",
      "/home/player/.local/share/org.cyberctf.desktop/labs/hello/.isoloom/docker/compose.yml",
      "/home/player/.local/share/org.cyberctf.desktop/labs/hello/.cyberctf-overlay.yml",
    ]);
    // A container without labels (`null` in Docker's JSON) is read all the same.
    expect(own).toMatchObject({ name: "postgres", refs: ["postgres"], paths: [] });
  });

  it("reads networks, volumes (a persistent one marked so) and images", () => {
    expect(names(parse("docker-network", "docker-network.txt"))).toEqual(["bridge", "cyberctf-hello_lab"]);
    expect(parse("docker-network", "docker-network.txt")[1].refs).toEqual(["cyberctf-hello_lab", "cyberctf-hello", "hello-stack"]);
    const volumes = parse("docker-volume", "docker-volume.txt");
    expect(names(volumes)).toEqual(["3f1bbda44747f314e8e2d9c0b32ccf622b76f35bc3e592b3a6a110fe63955eb8", "cyberctf-hello_data (persistent)"]);
    expect(volumes[1].key).toBe("cyberctf-hello_data");
    const images = parse("docker-image", "docker-image.txt");
    expect(names(images)).toEqual(["localhost/isoloom/hello-stack-web:latest", "busybox:1.37.0-musl"]);
    // Docker's "2026-11-11 10:11:19 +0100 CET".
    expect(images[0].created).toBe(epoch("2026-11-11T09:11:19Z"));
  });
});

describe("Vagrant", () => {
  it("lists every machine of its index with the folders it lives in", () => {
    const machines = parse("vagrant-machine", "vagrant-machine.json");
    expect(machines.map((m) => m.key)).toEqual(["f1358fc88e044266932b94aa1e2a5b70", "27cc6890c2294d8cb6ae436a8f0e1b2c"]);
    expect(machines[0].name).toBe("web (libvirt, /home/player/.local/share/org.cyberctf.desktop/labs/hello-vm/.isoloom/vagrant)");
    expect(machines[0].paths).toContain("/home/player/.local/share/org.cyberctf.desktop/runs/cyberctf-hello-vm/vagrant");
  });

  it("names a box by its name, version, architecture and provider", () => {
    expect(names(parse("vagrant-box", "vagrant-box.txt"))).toEqual(["bento/debian-12 202510.26.0 arm64 utm", "generic/alpine319 4.3.12 virtualbox"]);
  });

  it("reads an index that isn't there yet as no machine", () => {
    expect(parseProbe({ kind: "vagrant-machine", tool: "cat", status: "ok", note: "", output: "" }, 0)).toEqual([]);
  });
});

describe("processes and listening ports", () => {
  it("dates a process from ps's elapsed time and the snapshot's now, to the minute", () => {
    expect(elapsed("02-17:21:19")).toBe(2 * 86400 + 17 * 3600 + 21 * 60 + 19);
    expect(elapsed("01:49")).toBe(109);
    const [terraform, vagrant, worker] = parse("process", "process.ps.txt");
    expect(terraform.pid).toBe(8640);
    expect(vagrant.created).toBe(Math.floor((1_794_312_000 - 109) / 60) * 60);
    // The names that tie it to a run: a Compose project, a lab's cyberctf-…; its paths.
    expect(vagrant.refs).toEqual(["hello-stack"]);
    expect(worker.paths).toEqual(["/usr/bin/cyber-ctf-desktop", "/home/player/.local/share/org.cyberctf.desktop/deploys/hello.json"]);
    expect(terraform.refs).toEqual(["cyberctf-cloud"]);
  });

  it("reads Windows' processes from snapshot.ps1's JSON", () => {
    const [vmwp, worker] = parse("process", "process.windows.txt");
    expect(vmwp).toMatchObject({ pid: 4120, created: epoch("2026-11-11T12:00:00Z") });
    expect(worker.paths).toContain("C:\\Users\\vagrant\\AppData\\Roaming\\org.cyberctf.desktop\\deploys\\cyberctf-hello.json");
  });

  it("reads listening sockets from lsof (once a socket), ss and Windows", () => {
    expect(names(parse("listening-port", "listening-port.lsof.txt"))).toEqual([
      "*:7000 (ControlCenter, pid 645)",
      "127.0.0.1:8080 (qemu-system-aarch64, pid 48488)",
      "*:64810 (qemu-system-aarch64, pid 48488)",
    ]);
    const ss = parse("listening-port", "listening-port.ss.txt");
    expect(ss.map((p) => [p.key, p.pid])).toEqual([
      ["tcp 0.0.0.0:8080", 4242],
      ["tcp 0.0.0.0:22", 812],
      ["tcp 127.0.0.1:50022", 5151],
    ]);
    expect(parse("listening-port", "listening-port.windows.txt")[0]).toMatchObject({ key: "127.0.0.1:8080", pid: 5200 });
  });
});

describe("the network's configuration", () => {
  it("reads the hosts file's entries, comments left out", () => {
    expect(names(parse("hosts-entry", "hosts.txt"))).toEqual([
      "127.0.0.1 localhost",
      "255.255.255.255 broadcasthost",
      "::1 localhost",
      "10.20.0.5 web.hello.lab",
    ]);
  });

  it("keeps a routing table's network routes, never its host and cloned ones", () => {
    expect(names(parse("route", "route.netstat.txt"))).toEqual([
      "default 10.0.0.1 en0",
      "default link bridge100",
      "10/24 link en0",
      "10.0.0.1/32 link en0",
      "127 127.0.0.1 lo0",
      "192.168.64/24 link bridge100",
    ]);
    // ip's own churn (its source, metric, state) left out.
    expect(names(parse("route", "route.ip.txt"))).toEqual([
      "default via 192.168.121.1 dev eth0",
      "172.17.0.0/16 dev docker0",
      "10.20.0.0/24 dev br-ccad45594157",
      "192.168.121.0/24 dev virbr1",
    ]);
    expect(names(parse("route", "route.windows.txt"))).toEqual(["0.0.0.0/0 via 10.0.2.2 Ethernet", "172.24.80.0/20 via 0.0.0.0 vEthernet (Default Switch)"]);
  });

  it("reads pf's anchors, nftables' tables and iptables' rules without their counters", () => {
    expect(parse("pf-anchor", "pf-anchor.txt")).toHaveLength(4);
    expect(names(parse("nft-table", "nft-table.txt"))).toContain("table inet libvirt_network");
    expect(names(parse("iptables-rule", "iptables-save.txt"))).toEqual([
      "nat :PREROUTING ACCEPT",
      "nat :DOCKER -",
      "nat -A POSTROUTING -s 10.20.0.0/24 ! -o br-ccad45594157 -j MASQUERADE",
      "nat -A DOCKER -i br-ccad45594157 -j RETURN",
      "filter :INPUT ACCEPT",
      "filter :FORWARD DROP",
      "filter -A FORWARD -o br-ccad45594157 -j DOCKER",
    ]);
    expect(names(parse("portproxy", "portproxy.txt"))).toEqual(["127.0.0.1:8080 -> 172.24.85.12:80"]);
  });

  it("reads the credential store's items by service and account, on each system", () => {
    expect(names(parse("keychain-item", "keychain.security.txt"))).toEqual(["org.cyberctf.desktop session", "org.cyberctf.desktop server:3f2a9c1d0b8e7f65"]);
    expect(names(parse("keychain-item", "keychain.secret-tool.txt"))).toEqual(["org.cyberctf.desktop session", "org.cyberctf.desktop server:3f2a9c1d0b8e7f65"]);
    expect(names(parse("keychain-item", "keychain.cmdkey.txt"))).toEqual(["LegacyGeneric:target=session.org.cyberctf.desktop"]);
  });
});

describe("the hypervisors' objects", () => {
  it("keeps libvirt's system and session objects apart", () => {
    const domains = parse("libvirt-domain", "libvirt-domain.txt");
    expect(domains.map((d) => d.key)).toEqual(["system hello-vm_web", "system cyberctf-rig-debian", "session own-vm"]);
    expect(domains[0].refs).toEqual(["hello-vm_web"]);
    const volumes = parse("libvirt-volume", "libvirt-volume.txt");
    expect(volumes[0]).toMatchObject({ key: "system default/hello-vm_web.img", paths: ["/var/lib/libvirt/images/hello-vm_web.img"] });
  });

  it("reads VirtualBox's VMs, host-only adapters and networks, NAT networks and DHCP servers", () => {
    expect(parse("vbox-vm", "vbox-vm.txt").map((v) => [v.key, v.name])).toEqual([
      ["b683df88-ae9e-4e0f-af5f-7f0f04f01d4e", "debian-12.12-aarch64_1794132848692_83122"],
      ["1d2c3b4a-0000-4000-8000-000000000001", "hello-stack - web"],
    ]);
    expect(names(parse("vbox-hostonly", "vbox-hostonly.txt"))).toEqual(["vagrantnet-vbox1", "vboxnet0"]);
    expect(names(parse("vbox-natnet", "vbox-natnet.txt"))).toEqual(["NatNetwork"]);
    expect(names(parse("vbox-dhcp", "vbox-dhcp.txt"))).toEqual(["HostInterfaceNetworking-vboxnet0"]);
  });

  it("reads UTM's and Parallels' VMs by UUID", () => {
    expect(parse("utm-vm", "utm-vm.txt").map((v) => [v.key, v.name])).toEqual([
      ["6C7E3B58-2D4F-4C1A-9F65-1B2D3C4E5F60", "hello-stack - isoloom-docker-host"],
      ["A1B2C3D4-0000-4000-8000-000000000002", "Owner Windows 11"],
    ]);
    expect(names(parse("parallels-vm", "parallels-vm.txt"))).toEqual(["hello-stack - web"]);
  });

  it("reads Hyper-V's VMs and switches and Windows Firewall's rules from snapshot.ps1's JSON", () => {
    expect(parse("hyperv-vm", "hyperv-vm.txt")[0]).toMatchObject({
      key: "6b1a9a4c-1111-4222-8333-944455556666",
      name: "hello-stack-web",
      created: epoch("2026-11-11T12:00:05Z"),
    });
    expect(names(parse("hyperv-switch", "hyperv-switch.txt"))).toEqual(["Default Switch", "isoloom-hello-stack-lab"]);
    expect(parse("firewall-rule", "firewall-rule.txt")[0]).toMatchObject({ name: "Cyber CTF", refs: ["Cyber CTF"] });
    expect(names(parse("os-registration", "os-registration.txt"))).toEqual(["/home/player/.local/share/applications/cyber-ctf-desktop-handler.desktop"]);
  });
});

describe("a server's objects", () => {
  it("reads a Proxmox node's guests, SDN zones, users' tokens and volumes", () => {
    expect(parse("proxmox-vm", "proxmox-qm.txt").map((v) => [v.key, v.name])).toEqual([
      ["100", "100 iso1-web"],
      ["101", "101 iso1-isoloom-router"],
    ]);
    expect(names(parse("proxmox-sdn-zone", "proxmox-sdn-zone.json"))).toEqual(["iso1"]);
    expect(names(parse("proxmox-token", "proxmox-users.json"))).toEqual(["root@pam!rig-20261111-120000-proxmox-lab", "root@pam!cyberctf-1a2b3c4d"]);
    expect(names(parse("proxmox-user", "proxmox-users.json"))).toEqual(["root@pam", "isoloom@pve"]);
    expect(names(parse("proxmox-volume", "proxmox-pvesm.txt"))).toEqual(["local:iso/iso1-debian-12.img", "local-lvm:vm-100-disk-0"]);
  });

  it("reads an ESXi host's VMs, port groups, switches", () => {
    expect(parse("esxi-vm", "esxi-getallvms.txt")[0]).toMatchObject({ key: "1", name: "1 iso1-web", paths: ["[datastore1] iso1-web/iso1-web.vmx"] });
    expect(names(parse("esxi-portgroup", "esxi-portgroup.txt"))).toEqual(["Management Network (vSwitch0)", "VM Network (vSwitch0)", "iso1-lab (iso1-lab)"]);
    expect(names(parse("esxi-vswitch", "esxi-vswitch.txt"))).toEqual(["vSwitch0", "iso1-lab"]);
  });
});

describe("the shared readers", () => {
  it("reads a table by its columns' starts, blocks by their first field", () => {
    expect(table("ID   NAME\n1    a b\n")).toEqual([{ ID: "1", NAME: "a b" }]);
    expect(blocks("Name: a\nX: 1\nName: b\n\nName: c\n", "Name")).toEqual([{ Name: "a", X: "1" }, { Name: "b" }, { Name: "c" }]);
    expect(epoch("0001-01-01T00:00:00Z")).toBeUndefined();
    expect(epoch("not a date")).toBeUndefined();
  });

  it("refuses a file that isn't a snapshot, and a kind it has no parser for", () => {
    expect(() => readSnapshot("{}", "x.json")).toThrow("x.json isn't a leftover audit snapshot");
    expect(() => parseProbe({ kind: "teapot", tool: "", status: "ok", note: "", output: "" }, 0)).toThrow("no parser for the kind teapot");
    expect(Object.keys(PARSERS).length).toBeGreaterThan(40);
  });
});
