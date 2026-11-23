// The leftover audit's parsers of the hypervisors' and servers' objects: libvirt's, VirtualBox's,
// UTM's, Parallels', Hyper-V's and Windows Firewall's (snapshot.ps1's JSON lines), a Proxmox node's
// and an ESXi host's (snapshot-server.sh).

import type { Item, Parser } from "./inventory.ts";
import { blocks, epoch, jsonLines, lines, refs, table } from "./parse-util.ts";

const str = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" ? String(v) : "");

/** An item named by its key, tied to a run by `name`. */
const named = (kind: string, key: string, name: string, extra: Partial<Item> = {}): Item => ({ kind, key, name, refs: refs(name), paths: [], ...extra });

/** libvirt's objects, `<connection> <name>` a line (`<connection> <pool> <volume> <path>` for a
 *  volume): the connection keeps the system's apart from the session's. */
const libvirt =
  (kind: string): Parser =>
  (output) =>
    lines(output).map((l) => {
      const [conn, ...rest] = l.trim().split(/\s+/);
      if (kind === "libvirt-volume") {
        const [pool, volume, ...path] = rest;
        return named(kind, `${conn} ${pool}/${volume}`, volume, { name: `${pool}/${volume} (${conn})`, refs: refs(volume), paths: refs(path.join(" ")) });
      }
      const name = rest.join(" ");
      return named(kind, `${conn} ${name}`, name, { name: `${name} (${conn})`, refs: refs(name) });
    });

/** VirtualBox's VMs, `"<name>" {<uuid>}` a line. */
const vboxVms: Parser = (output) =>
  lines(output).flatMap((l) => {
    const m = /^"(.*)" \{([^}]+)\}$/.exec(l.trim());
    return m ? [named("vbox-vm", m[2], m[1])] : [];
  });

/** VirtualBox's networks of a kind, as the blocks its `list` prints, named by `field`. */
const vboxBlocks =
  (kind: string, field: string): Parser =>
  (output) =>
    blocks(output, field).map((b) => named(kind, b[field], b[field]));

/** Host-only adapters (`list hostonlyifs`, `Name:`) and, on a Mac, host-only networks
 *  (`list hostonlynets`, `Name:` too). */
const vboxHostOnly: Parser = (output) => vboxBlocks("vbox-hostonly", "Name")(output, 0);

/** NAT networks: `NetworkName:` on VirtualBox 6 and 7.0, `Name:` on 7.1. */
const vboxNatNets: Parser = (output) => {
  const field = /^NetworkName:/m.test(output) ? "NetworkName" : "Name";
  return vboxBlocks("vbox-natnet", field)(output, 0);
};

/** VMware's virtual networks, from its `networking` file (Fusion's
 *  `/Library/Preferences/VMware Fusion/networking`, Workstation's `/etc/vmware/networking` on
 *  Linux): each `answer VNET_<n>_<setting> <value>` line a setting of network `vmnet<n>`, named
 *  with its subnet and whether it is host-only or NAT, so one made or readdressed reads as new. */
const vmwareNetworks: Parser = (output) => {
  const nets = new Map<string, Map<string, string>>();
  for (const l of lines(output)) {
    const m = /^answer VNET_(\d+)_(\w+) (.*)$/.exec(l.trim());
    if (!m) continue;
    const settings = nets.get(m[1]) ?? new Map<string, string>();
    settings.set(m[2], m[3].trim());
    nets.set(m[1], settings);
  }
  return [...nets].map(([n, s]) => {
    const subnet = s.get("HOSTONLY_SUBNET");
    const mask = s.get("HOSTONLY_NETMASK");
    const kind = s.get("NAT") === "yes" ? "nat" : "host-only";
    return named("vmware-network", `vmnet${n}`, subnet ? `vmnet${n} ${subnet}/${mask ?? "?"} ${kind}` : `vmnet${n} ${kind}`);
  });
};

/** A table whose rows carry a UUID and a name (`utmctl list`, `prlctl list -a -o uuid,name`). */
const uuidTable =
  (kind: string): Parser =>
  (output) =>
    table(output).flatMap((r) => {
      const id = r.UUID || r.ID;
      const name = r.Name || r.NAME;
      return id ? [named(kind, id, name)] : [];
    });

/** A Windows probe's JSON lines: each object's `keyField` its key, `nameField` its name. */
const winJson =
  (kind: string, keyField: string, nameField: string, extra: (o: Record<string, unknown>) => Partial<Item> = () => ({})): Parser =>
  (output) =>
    jsonLines(output).map((o) => named(kind, str(o[keyField]), str(o[nameField]), extra(o)));

/** `qm list` (`VMID NAME STATUS …`) and `pct list` (`VMID Status Lock Name`, its lock maybe
 *  empty): a Proxmox node's VMs and containers, by id. Their names have no spaces. */
const pveGuests =
  (kind: string): Parser =>
  (output) =>
    lines(output).flatMap((l) => {
      const f = l.trim().split(/\s+/);
      if (!/^\d+$/.test(f[0])) return [];
      const name = kind === "proxmox-ct" ? f[f.length - 1] : f[1];
      return [named(kind, f[0], `${f[0]} ${name}`, { refs: refs(name) })];
    });

/** `pvesh get … --output-format json`: an array, each object's `field` its key. */
const pveJson =
  (kind: string, field: string): Parser =>
  (output) =>
    output.trim() === "" ? [] : (JSON.parse(output) as Record<string, unknown>[]).map((o) => named(kind, str(o[field]), str(o[field])));

/** The API tokens of every user (`pvesh get /access/users --full 1`): `<user>!<token>`. */
const pveTokens: Parser = (output) => {
  if (output.trim() === "") return [];
  const users = JSON.parse(output) as { userid: string; tokens?: { tokenid: string }[] }[];
  return users.flatMap((u) =>
    (u.tokens ?? []).map((t) => named("proxmox-token", `${u.userid}!${t.tokenid}`, `${u.userid}!${t.tokenid}`, { refs: refs(t.tokenid) })),
  );
};

/** `pvesm list <storage>` for each storage, `<storage> ` before each line: the disks and images. */
const pveVolumes: Parser = (output) =>
  lines(output).flatMap((l) => {
    const volid = l.trim().split(/\s+/)[1] as string | undefined;
    if (!volid || volid === "Volid") return [];
    return [named("proxmox-volume", volid, volid, { refs: refs(volid.split(/[:/]/).pop()) })];
  });

/** `netsh interface portproxy show v4tov4`: `<listen address> <port> <connect address> <port>`. */
const portProxies: Parser = (output) =>
  lines(output).flatMap((l) => {
    const m = /^(\S+)\s+(\d+)\s+(\S+)\s+(\d+)\s*$/.exec(l);
    return m ? [named("portproxy", `${m[1]}:${m[2]}`, `${m[1]}:${m[2]} -> ${m[3]}:${m[4]}`, { refs: [] })] : [];
  });

/** `vim-cmd vmsvc/getallvms`: an ESXi host's VMs, by id (`Vmid Name File Guest OS …`). */
const esxiVms: Parser = (output) =>
  lines(output).flatMap((l) => {
    const m = /^(\d+)\s+(.+?)\s+(\[[^\]]+\] \S+)/.exec(l);
    return m ? [named("esxi-vm", m[1], `${m[1]} ${m[2]}`, { refs: refs(m[2]), paths: [m[3]] })] : [];
  });

/** `esxcli network vswitch standard portgroup list`: a table under a dashed rule. */
const esxiPortGroups: Parser = (output) =>
  lines(output)
    .filter((l) => !/^(Name\s|-+)/.test(l))
    .flatMap((l) => {
      const m = /^(.+?)\s{2,}(\S+)\s+\d+\s+\d+\s*$/.exec(l);
      return m ? [named("esxi-portgroup", m[1], `${m[1]} (${m[2]})`, { refs: refs(m[1]) })] : [];
    });

/** `esxcli network vswitch standard list`: each switch's block starts with its name, unindented. */
const esxiSwitches: Parser = (output) =>
  lines(output)
    .filter((l) => !/^\s/.test(l))
    .map((l) => named("esxi-vswitch", l.trim(), l.trim()));

export const MACHINE_PARSERS: Record<string, Parser> = {
  "libvirt-domain": libvirt("libvirt-domain"),
  "libvirt-network": libvirt("libvirt-network"),
  "libvirt-pool": libvirt("libvirt-pool"),
  "libvirt-volume": libvirt("libvirt-volume"),
  "vbox-vm": vboxVms,
  "vbox-hostonly": vboxHostOnly,
  "vbox-natnet": vboxNatNets,
  "vbox-dhcp": vboxBlocks("vbox-dhcp", "NetworkName"),
  "utm-vm": uuidTable("utm-vm"),
  "parallels-vm": uuidTable("parallels-vm"),
  "vmware-network": vmwareNetworks,
  "hyperv-vm": winJson("hyperv-vm", "Id", "Name", (o) => ({ created: epoch(o.CreationTime), paths: refs(o.Path) })),
  "hyperv-switch": winJson("hyperv-switch", "Id", "Name"),
  "hyperv-nat": winJson("hyperv-nat", "Name", "Name"),
  "hyperv-vhd": winJson("hyperv-vhd", "Path", "Path", (o) => ({ paths: refs(o.Path), created: epoch(o.CreationTime) })),
  portproxy: portProxies,
  "firewall-rule": winJson("firewall-rule", "Name", "DisplayName", (o) => ({ refs: refs(o.DisplayName, o.Group) })),
  "proxmox-vm": pveGuests("proxmox-vm"),
  "proxmox-ct": pveGuests("proxmox-ct"),
  "proxmox-sdn-zone": pveJson("proxmox-sdn-zone", "zone"),
  "proxmox-sdn-vnet": pveJson("proxmox-sdn-vnet", "vnet"),
  "proxmox-network": pveJson("proxmox-network", "iface"),
  "proxmox-pool": pveJson("proxmox-pool", "poolid"),
  "proxmox-user": pveJson("proxmox-user", "userid"),
  "proxmox-token": pveTokens,
  "proxmox-ipset": pveJson("proxmox-ipset", "name"),
  "proxmox-mapping": pveJson("proxmox-mapping", "id"),
  "proxmox-volume": pveVolumes,
  "esxi-vm": esxiVms,
  "esxi-portgroup": esxiPortGroups,
  "esxi-vswitch": esxiSwitches,
  "esxi-file": (o) => lines(o).map((l) => named("esxi-file", l.trim(), l.trim(), { refs: refs(l.trim().split("/").pop()), paths: [l.trim()] })),
};
