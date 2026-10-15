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
  type ZoneData,
} from "@/features/labs/network-diagram/model";

// The lab as a graph (zones, machines, links), before layout.

export type Topology = { nodes: Node[]; edges: Edge<LinkData>[]; zones: { id: string; members: string[]; data: ZoneData }[] };

/** The graph as it is built: nodes and edges in drawing order, and each network's members. */
type Graph = { nodes: Node[]; edges: Edge<LinkData>[]; members: Map<string, string[]> };

/** A network's members, as a list kept in the graph (made when the network has none yet). */
function membersOf(g: Graph, network: string): string[] {
  const list = g.members.get(network) ?? [];
  g.members.set(network, list);
  return list;
}

/**
 * Networks: as reported, plus any a machine names that wasn't (best effort), or a single
 * synthetic one for runtimes that report no networks (its subnet guessed from an address).
 * Copies: a guessed subnet must not be written into the caller's (polled) networks. Returns
 * them by name, and each machine's interfaces (one on the first network when it reports none).
 */
function labNetworks(machines: Machine[], networks: LabNetwork[]) {
  const nets = new Map<string, LabNetwork>(networks.map((n) => [n.name, { ...n }]));
  const ifacesOf = (m: Machine): LabInterface[] => {
    if (m.interfaces.length) return m.interfaces;
    const fallback = nets.keys().next().value ?? "default";
    return [{ network: fallback, ip: m.ip }];
  };
  machines.forEach((m) => {
    ifacesOf(m).forEach((i) => nets.has(i.network) || nets.set(i.network, { name: i.network, subnet: "", internal: false }));
  });
  if (nets.size === 0) nets.set("default", { name: "default", subnet: "", internal: false });
  for (const n of nets.values()) {
    if (!n.subnet) {
      const ip = machines.flatMap(ifacesOf).find((i) => i.network === n.name && isIp(i.ip))?.ip;
      if (ip) n.subnet = `${ip.split(".").slice(0, 2).join(".")}.0.0/16`;
    }
  }
  return { nets, ifacesOf };
}

/** Distance of each network from `entry` (where the attacker enters), hopping through pivots, so
 *  the tree reads top-down in attack order: attack box → entry network → pivot → deeper network.
 *  A network out of reach is far (99). */
function networkDistance(entry: string, machines: Machine[], ifacesOf: (m: Machine) => LabInterface[]) {
  const dist = new Map<string, number>([[entry, 0]]);
  for (let frontier = [entry]; frontier.length;) {
    const next: string[] = [];
    for (const net of frontier) {
      for (const m of machines) {
        const on = ifacesOf(m).map((i) => i.network);
        if (!on.includes(net)) continue;
        for (const other of on.filter((o) => !dist.has(o))) {
          dist.set(other, (dist.get(net) ?? 0) + 1);
          next.push(other);
        }
      }
    }
    frontier = next;
  }
  return (net: string) => dist.get(net) ?? 99;
}

/** One bridge per network; a network with a route out (not compose `internal`) is wired up to
 *  the host: its bridge's uplink runs up under the card header. An isolated network gets none. */
function addBridges(g: Graph, ordered: LabNetwork[]) {
  ordered.forEach((n) =>
    g.nodes.push({
      id: `bridge-${n.name}`,
      type: "bridge",
      position: { x: 0, y: 0 },
      style: { width: BRIDGE.w, height: BRIDGE.h },
      data: { label: translate("diagram.bridge", { name: netLabel(n.name) }) },
      draggable: false,
    }),
  );
  ordered
    .filter((n) => !n.internal)
    .forEach((n) => {
      g.nodes.push({
        id: `up-${n.name}`,
        type: "uplink",
        position: { x: 0, y: 0 },
        style: { width: ANCHOR, height: ANCHOR },
        data: {},
        draggable: false,
        selectable: false,
      });
      g.edges.push({ ...link(`e-up-${n.name}`, `up-${n.name}`, `bridge-${n.name}`, GREY), targetHandle: "up" });
    });
}

/** The attack box is a host on the lab network like the others (one attack box can be attached
 *  to several running labs at once), first in its zone so it reads leftmost. */
function addAttacker(g: Graph, attacker: NonNullable<Attacker>, entry: string) {
  const atkOn = attacker.running;
  g.nodes.push({
    id: "__attacker",
    type: "attacker",
    position: { x: 0, y: 0 },
    data: {
      label: translate("diagram.attacker.label"),
      subtitle: atkOn ? attacker.ip || translate("diagram.attacker.attached") : translate("diagram.attacker.notAttached"),
      running: atkOn,
    },
  });
  membersOf(g, entry).push("__attacker");
  g.edges.push(link("e-attach", `bridge-${entry}`, "__attacker", atkOn ? attack : IDLE, { animated: atkOn }));
}

/** A machine: its card, in its network's zone (single-homed) or between zones (a pivot plugs
 *  into its nearest network(s) and leads on into the deeper ones, each link carrying its address
 *  on that network), then its localhost port bindings. */
function addMachine(g: Graph, m: Machine, ifaces: LabInterface[], d: (net: string) => number) {
  const type = serviceType(m.image, m.name);
  const id = `svc-${m.name}`;
  g.nodes.push({
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
    membersOf(g, ifaces[0].network).push(id);
    g.edges.push(link(`e-${ifaces[0].network}-${m.name}`, `bridge-${ifaces[0].network}`, id, GREY));
  } else {
    const near = Math.min(...ifaces.map((i) => d(i.network)));
    // Two NICs on one network are two links: the second id gets its interface's index.
    const taken = (edgeId: string, n: number) => (g.edges.some((e) => e.id === edgeId) ? `${edgeId}-${String(n)}` : edgeId);
    ifaces.forEach((i, n) =>
      g.edges.push(
        d(i.network) === near
          ? link(taken(`e-${i.network}-${m.name}`, n), `zone-${i.network}`, id, GREY, { sideways: true, label: i.ip })
          : link(taken(`e-${m.name}-${i.network}`, n), id, `zone-${i.network}`, PIVOT, { sideways: true, label: i.ip }),
      ),
    );
  }
  addHostPorts(g, m, id, type);
}

/** Localhost port bindings: a small node below, linked from the container it forwards to. */
function addHostPorts(g: Graph, m: Machine, id: string, type: ReturnType<typeof serviceType>) {
  uniquePorts(m.ports)
    .filter((p) => p.published > 0)
    .forEach((p) => {
      const hp = `hp-${m.name}-${String(p.published)}`;
      g.nodes.push({
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
      g.edges.push(shown ? { ...e, sourceHandle: publishHandle(p.published) } : e);
    });
}

/** The lab as a graph: zones (networks) holding bridges and single-homed machines, pivots
 *  between zones, the attack zone on top and localhost bindings below. Positions come later. */
export function topology(machines: Machine[], networks: LabNetwork[], attacker: Attacker): Topology {
  const { nets, ifacesOf } = labNetworks(machines, networks);
  const entry = (attacker?.labNetwork && nets.has(attacker.labNetwork) && attacker.labNetwork) || (nets.has("default") ? "default" : [...nets.keys()][0]);
  const d = networkDistance(entry, machines, ifacesOf);

  // One zone per lab network, nearest first.
  const ordered = [...nets.values()].sort((a, b) => d(a.name) - d(b.name) || a.name.localeCompare(b.name));
  const g: Graph = { nodes: [], edges: [], members: new Map(ordered.map((n) => [n.name, [`bridge-${n.name}`]])) };
  addBridges(g, ordered);
  // No attack box at all (VM labs) draws none.
  if (attacker) addAttacker(g, attacker, entry);
  // The provisioning controller is plumbing, not a target: it has no place on the map.
  machines
    .filter((m) => !m.infra)
    .forEach((m) => {
      addMachine(g, m, ifacesOf(m), d);
    });

  const zones: Topology["zones"] = ordered.map((n) => ({
    id: `zone-${n.name}`,
    members: membersOf(g, n.name),
    data: { label: translate("diagram.zone.network", { name: netLabel(n.name) }).toUpperCase(), detail: n.subnet, isolated: n.internal },
  }));
  return { nodes: g.nodes, edges: g.edges, zones };
}
