import { type Edge, type Node } from "@xyflow/react";
import type { LabInterface, LabNetwork } from "@/lib/tauri";
import { link } from "@/features/labs/network-diagram/edges";
import { translate } from "@/lib/i18n";
import {
  ANCHOR,
  Attacker,
  BRIDGE,
  ComputerData,
  GREEN,
  GREY,
  IDLE,
  LinkData,
  Machine,
  PIVOT,
  attack,
  isIp,
  netLabel,
  publishHandle,
  serviceLabel,
  serviceRows,
  serviceType,
  uniquePorts,
} from "@/features/labs/network-diagram/model";

// The lab as a graph (zones, machines, links), before layout.

export type Topology = { nodes: Node[]; edges: Edge<LinkData>[]; zones: { id: string; members: string[]; data: Record<string, unknown> }[] };

/** The lab as a graph: zones (networks) holding bridges and single-homed machines, pivots
 *  between zones, the attack zone on top and localhost bindings below. Positions come later. */
export function topology(machines: Machine[], networks: LabNetwork[], attacker: Attacker): Topology {
  const atkOn = !!attacker?.running;

  // Networks: as reported, plus any a machine names that wasn't (best effort), or a single
  // synthetic one for runtimes that report no networks (its subnet guessed from an address).
  // Copies: a guessed subnet below must not be written into the caller's (polled) networks.
  const nets = new Map<string, LabNetwork>(networks.map((n) => [n.name, { ...n }]));
  const ifacesOf = (m: Machine): LabInterface[] => {
    if (m.interfaces?.length) return m.interfaces;
    const fallback = nets.keys().next().value ?? "default";
    return [{ network: fallback, ip: m.ip }];
  };
  machines.forEach((m) => ifacesOf(m).forEach((i) => nets.has(i.network) || nets.set(i.network, { name: i.network, subnet: "", internal: false })));
  if (nets.size === 0) nets.set("default", { name: "default", subnet: "", internal: false });
  for (const n of nets.values()) {
    if (!n.subnet) {
      const ip = machines.flatMap(ifacesOf).find((i) => i.network === n.name && isIp(i.ip))?.ip;
      if (ip) n.subnet = `${ip.split(".").slice(0, 2).join(".")}.0.0/16`;
    }
  }

  // Distance of each network from where the attacker enters, hopping through pivots, so the
  // tree reads top-down in attack order: attack box → entry network → pivot → deeper network.
  const entry = (attacker?.labNetwork && nets.has(attacker.labNetwork) && attacker.labNetwork) || (nets.has("default") ? "default" : [...nets.keys()][0]);
  const dist = new Map<string, number>([[entry, 0]]);
  for (let frontier = [entry]; frontier.length;) {
    const next: string[] = [];
    for (const net of frontier) {
      for (const m of machines) {
        const on = ifacesOf(m).map((i) => i.network);
        if (!on.includes(net)) continue;
        for (const other of on.filter((o) => !dist.has(o))) {
          dist.set(other, dist.get(net)! + 1);
          next.push(other);
        }
      }
    }
    frontier = next;
  }
  const d = (net: string) => dist.get(net) ?? 99;

  const nodes: Node[] = [];
  const edges: Edge<LinkData>[] = [];
  const zones: Topology["zones"] = [];

  // One zone per lab network, nearest first.
  const ordered = [...nets.values()].sort((a, b) => d(a.name) - d(b.name) || a.name.localeCompare(b.name));
  const members = new Map<string, string[]>(ordered.map((n) => [n.name, [`bridge-${n.name}`]]));
  ordered.forEach((n) =>
    nodes.push({
      id: `bridge-${n.name}`,
      type: "bridge",
      position: { x: 0, y: 0 },
      style: { width: BRIDGE.w, height: BRIDGE.h },
      data: { label: translate("diagram.bridge", { name: netLabel(n.name) }) },
      draggable: false,
    }),
  );

  // A network with a route out (not compose `internal`) is wired up to the host: its bridge's
  // uplink runs up under the card header. An isolated network gets none.
  ordered
    .filter((n) => !n.internal)
    .forEach((n) => {
      nodes.push({
        id: `up-${n.name}`,
        type: "uplink",
        position: { x: 0, y: 0 },
        style: { width: ANCHOR, height: ANCHOR },
        data: {},
        draggable: false,
        selectable: false,
      });
      edges.push({ ...link(`e-up-${n.name}`, `up-${n.name}`, `bridge-${n.name}`, GREY), targetHandle: "up" });
    });

  // The attack box is a host on the lab network like the others (one attack box can be
  // attached to several running labs at once), first in its zone so it reads leftmost.
  // No attack box at all (VM labs) draws none.
  if (attacker) {
    nodes.push({
      id: "__attacker",
      type: "attacker",
      position: { x: 0, y: 0 },
      data: {
        label: translate("diagram.attacker.label"),
        subtitle: atkOn ? attacker.ip || translate("diagram.attacker.attached") : translate("diagram.attacker.notAttached"),
        running: atkOn,
      },
    });
    members.get(entry)!.push("__attacker");
    edges.push(link("e-attach", `bridge-${entry}`, "__attacker", atkOn ? attack : IDLE, { animated: atkOn }));
  }

  // The provisioning controller is plumbing, not a target: it has no place on the map.
  machines
    .filter((m) => !m.infra)
    .forEach((m) => {
      const ifaces = ifacesOf(m);
      const type = serviceType(m.image, m.name);
      const id = `svc-${m.name}`;
      nodes.push({
        id,
        type: "computer",
        position: { x: 0, y: 0 },
        data: {
          hostname: m.name,
          image: m.image || serviceLabel(type),
          type,
          ifaces,
          running: m.state === "running",
          state: m.state,
          ports: uniquePorts(m.ports),
          rows: serviceRows(m, uniquePorts(m.ports), type),
        } satisfies ComputerData,
      });
      if (ifaces.length === 1) {
        members.get(ifaces[0].network)!.push(id);
        edges.push(link(`e-${ifaces[0].network}-${m.name}`, `bridge-${ifaces[0].network}`, id, GREY));
      } else {
        // A pivot plugs into its nearest network(s) and leads on into the deeper ones; each
        // link carries its address on that network.
        const near = Math.min(...ifaces.map((i) => d(i.network)));
        ifaces.forEach((i) =>
          edges.push(
            d(i.network) === near
              ? link(`e-${i.network}-${m.name}`, `zone-${i.network}`, id, GREY, { sideways: true, label: i.ip })
              : link(`e-${m.name}-${i.network}`, id, `zone-${i.network}`, PIVOT, { sideways: true, label: i.ip }),
          ),
        );
      }
      // Localhost port bindings: a small node below, linked from the container it forwards to.
      uniquePorts(m.ports)
        .filter((p) => p.published > 0)
        .forEach((p) => {
          const hp = `hp-${m.name}-${p.published}`;
          nodes.push({
            id: hp,
            type: "hostport",
            position: { x: 0, y: 0 },
            style: { width: ANCHOR, height: ANCHOR },
            data: { port: p.published },
            draggable: false,
            selectable: false,
          });
          // From the service that publishes it (its box's right edge); a port no service row
          // shows keeps the card's own bottom handle.
          const shown = serviceRows(m, uniquePorts(m.ports), type).some((r) => r.ports.some((q) => q.published === p.published));
          const e = link(`e-${hp}`, id, hp, GREEN, { dashed: true, label: translate("diagram.published") });
          edges.push(shown ? { ...e, sourceHandle: publishHandle(p.published) } : e);
        });
    });

  ordered.forEach((n) => {
    const ids = members.get(n.name)!;
    zones.push({
      id: `zone-${n.name}`,
      members: ids,
      data: { label: translate("diagram.zone.network", { name: netLabel(n.name) }).toUpperCase(), detail: n.subnet, isolated: n.internal },
    });
  });

  return { nodes, edges, zones };
}
