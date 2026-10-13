import { type Edge } from "@xyflow/react";
import type { ElkExtendedEdge, ElkNode } from "elkjs/lib/elk-api";
import { elkEngine } from "@/features/labs/network-diagram/elk-engine";
import { zoneMinWidth } from "@/features/labs/network-diagram/frame";
import { LinkData, MIN_ZOOM, Pt } from "@/features/labs/network-diagram/model";
import { Topology } from "@/features/labs/network-diagram/topology";

// ELK layout of the topology; published ports are placed after it, along the card's bottom edge.

/** The room one published-port tab takes along the bottom row, in layout units. The tab is
 *  drawn at its own size (~80px with a gap) whatever the zoom, so the slot is sized for the
 *  smallest zoom the diagram is drawn at. */
export const PORT_TAB_SLOT = Math.ceil(88 / MIN_ZOOM);
/** A published-port lane beside its card: the first one this far out, the next ones a step further. */
const LANE_GAP = 14;
const LANE_STEP = 10;

type Size = (id: string) => { w: number; h: number };
type HandleAt = (node: string, handle: string) => Pt | undefined;
type Box = { x: number; y: number; w: number; h: number };

const SPACING = {
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.layered.spacing.edgeNodeBetweenLayers": "20",
  "elk.layered.spacing.edgeEdgeBetweenLayers": "12",
  "elk.spacing.edgeNode": "18",
  "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
  "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
};

/** Published ports and uplinks are placed after ELK, around the laid-out lab. */
const isHostPort = (id: string) => id.startsWith("hp-") || id.startsWith("up-");

/** Published ports leave from their service's box (its right edge), each down its own lane
 *  beside the card: the number of lanes per card, and the room kept for them on its right, so
 *  they never run into a neighbour. */
function portLanes(topo: Topology) {
  const lanes = new Map<string, number>();
  topo.edges.filter((e) => e.sourceHandle?.startsWith("pub-")).forEach((e) => lanes.set(e.source, (lanes.get(e.source) ?? 0) + 1));
  const laneRoom = (id: string) => (lanes.has(id) ? LANE_GAP + lanes.get(id)! * LANE_STEP : 0);
  return { lanes, laneRoom };
}

/** Every node gets two fixed ports, top-centre in and bottom-centre out, matching the React Flow
 *  handles, so ELK's routes end exactly on the dots. Pivots (top-level machines) also get
 *  mid-height side ports, west in and east out. */
function leaf(id: string, size: Size, laneRoom: (id: string) => number, sides = false): ElkNode {
  const { w, h } = size(id);
  return {
    id,
    width: w + laneRoom(id),
    height: h,
    layoutOptions: { "elk.portConstraints": "FIXED_POS" },
    ports: [
      { id: `${id}:in`, x: w / 2, y: 0, width: 0, height: 0, layoutOptions: { "elk.port.side": "NORTH" } },
      { id: `${id}:out`, x: w / 2, y: h, width: 0, height: 0, layoutOptions: { "elk.port.side": "SOUTH" } },
      ...(sides
        ? [
            { id: `${id}:w`, x: 0, y: h / 2, width: 0, height: 0, layoutOptions: { "elk.port.side": "WEST" } },
            { id: `${id}:e`, x: w, y: h / 2, width: 0, height: 0, layoutOptions: { "elk.port.side": "EAST" } },
          ]
        : []),
    ],
  };
}

/** ELK port for an edge end: a side handle ("w"/"e") or the default top/bottom one. */
const port = (node: string, handle: string | null | undefined, fallback: "in" | "out") => `${node}:${handle ?? fallback}`;

/** The topology as an ELK graph: zones left to right, each laid out top-down, the pivots
 *  between them; host ports are left out. */
function elkGraph(topo: Topology, size: Size, laneRoom: (id: string) => number): ElkNode {
  const inZone = new Set(topo.zones.flatMap((z) => z.members));
  return {
    id: "root",
    layoutOptions: {
      ...SPACING,
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.hierarchyHandling": "SEPARATE_CHILDREN",
      "elk.layered.spacing.nodeNodeBetweenLayers": "64",
      "elk.spacing.nodeNode": "40",
      "elk.padding": "[top=28,left=24,bottom=24,right=24]",
      "elk.json.shapeCoords": "ROOT",
      "elk.json.edgeCoords": "ROOT",
    },
    children: [
      ...topo.zones.map((z) => ({
        id: z.id,
        layoutOptions: {
          ...SPACING,
          "elk.algorithm": "layered",
          "elk.direction": "DOWN",
          "elk.layered.spacing.nodeNodeBetweenLayers": "40",
          "elk.spacing.nodeNode": "24",
          "elk.padding": "[top=20,left=20,bottom=20,right=20]",
          "elk.portConstraints": "FIXED_SIDE",
          // Room for the label beside the bridge, reserved here so ELK keeps the pivots and the
          // other segments clear of it; the contents stay centred in a widened segment.
          "elk.nodeSize.constraints": "MINIMUM_SIZE",
          "elk.nodeSize.minimum": `(${zoneMinWidth(z.data, size(z.members[0]).w)}, 0)`,
          "elk.contentAlignment": "H_CENTER V_TOP",
        },
        children: z.members.map((id) => leaf(id, size, laneRoom)),
        // A segment's side ports: ELK centres a lone port on its side, matching the handles.
        ports: [
          { id: `${z.id}:w`, layoutOptions: { "elk.port.side": "WEST" } },
          { id: `${z.id}:e`, layoutOptions: { "elk.port.side": "EAST" } },
        ],
      })),
      ...topo.nodes.filter((n) => !inZone.has(n.id) && !isHostPort(n.id)).map((n) => leaf(n.id, size, laneRoom, true)),
    ],
    edges: topo.edges
      .filter((e) => !isHostPort(e.target) && !isHostPort(e.source))
      .map((e) => ({ id: e.id, sources: [port(e.source, e.sourceHandle, "out")], targets: [port(e.target, e.targetHandle, "in")] })),
  };
}

/** The laid-out boxes (without their lane room) and edge routes, by id. */
function readLayout(out: ElkNode, laneRoom: (id: string) => number) {
  const boxes = new Map<string, Box>();
  const walk = (n: ElkNode) => {
    if (n.id !== "root") boxes.set(n.id, { x: n.x ?? 0, y: n.y ?? 0, w: (n.width ?? 0) - laneRoom(n.id), h: n.height ?? 0 });
    n.children?.forEach(walk);
  };
  walk(out);
  const routes = new Map<string, Pt[]>();
  const collect = (n: ElkNode) => {
    (n.edges as ElkExtendedEdge[] | undefined)?.forEach((e) => {
      const s = e.sections?.[0];
      if (s) routes.set(e.id, [s.startPoint, ...(s.bendPoints ?? []), s.endPoint]);
    });
    n.children?.forEach(collect);
  };
  collect(out);
  return { boxes, routes };
}

/** A published port's tab: its link, its machine's box, where its tab goes along the row, its
 *  size, where its link leaves the machine (a service's handle) and its lane. */
type Placed = { e: Edge<LinkData>; m: Box; x: number; w: number; h: number; start?: Pt; lane: number };

/**
 * Where each published port's tab goes along the bottom row. Each tab is ~72px wide (":51198"
 * plus its icon) and is drawn centred on its anchor, so anchors need a tab's width between them,
 * under one machine and across neighbouring machines alike: centred under its machine, then
 * pushed right along the row wherever it would overlap the previous one.
 */
function placePortTabs(topo: Topology, boxes: Map<string, Box>, size: Size, handleAt?: HandleAt): Placed[] {
  const byMachine = new Map<string, Edge<LinkData>[]>();
  topo.edges.filter((e) => isHostPort(e.target)).forEach((e) => byMachine.set(e.source, [...(byMachine.get(e.source) ?? []), e]));
  const placed: Placed[] = [];
  byMachine.forEach((list, machine) => {
    const m = boxes.get(machine);
    if (!m) return;
    const items = list.map((e) => {
      const h = e.sourceHandle ? handleAt?.(machine, e.sourceHandle) : undefined;
      return { e, start: h ? { x: m.x + h.x, y: m.y + h.y } : undefined };
    });
    // Untangled: the lowest service takes the innermost lane, turns first and gets the leftmost
    // tab; higher ones go further out and lower, so no two links cross. Links from the card's
    // bottom (a port no service shows) come first, straight down on the left.
    const fromBottom = items.filter((i) => !i.start);
    const fromServices = items.filter((i) => i.start).sort((a, b) => b.start!.y - a.start!.y);
    [...fromBottom, ...fromServices].forEach((it, k, all) => {
      const { w, h } = size(it.e.target);
      placed.push({
        e: it.e,
        m,
        w,
        h,
        start: it.start,
        lane: it.start ? fromServices.indexOf(it) : -1,
        x: m.x + m.w / 2 - w / 2 + (k - (all.length - 1) / 2) * PORT_TAB_SLOT,
      });
    });
  });
  placed.sort((a, b) => a.x - b.x);
  placed.forEach((p, i) => {
    if (i > 0) p.x = Math.max(p.x, placed[i - 1].x + PORT_TAB_SLOT);
  });
  return placed;
}

/** A published port's link down to its tab on the row at `rowY`: from its service, out along its
 *  lane and under the card; from the card's bottom, straight down then across. */
function tabRoute({ m, x, w, start, lane }: Placed, rowY: number): Pt[] {
  const tab = x + w / 2;
  if (start) {
    const laneX = m.x + m.w + LANE_GAP + lane * LANE_STEP;
    const turnY = m.y + m.h + LANE_GAP + lane * LANE_STEP;
    return [start, { x: laneX, y: start.y }, { x: laneX, y: turnY }, { x: tab, y: turnY }, { x: tab, y: rowY }];
  }
  return [
    { x: m.x + m.w / 2, y: m.y + m.h },
    { x: m.x + m.w / 2, y: rowY - 24 },
    { x: tab, y: rowY - 24 },
    { x: tab, y: rowY },
  ];
}

/** Lays the measured nodes out with ELK. Zones run left to right in attack order (attack →
 *  entry network → pivot → deeper network); inside a zone the bridge sits above its machines.
 *  Localhost bindings are left out of ELK and set in a band along the bottom (the host edge).
 *  Returns absolute positions, zone frames and routes. */
export async function layout(topo: Topology, size: Size, handleAt?: HandleAt) {
  const { lanes, laneRoom } = portLanes(topo);
  const out = await (await elkEngine()).layout(elkGraph(topo, size, laneRoom));
  const { boxes, routes } = readLayout(out, laneRoom);

  // Published ports: a row below everything, each under its container (side by side when
  // it has several), linked up to it. The view docks this row on the card's bottom edge.
  const laid = [...boxes.values()];
  let minX = Math.min(...laid.map((b) => b.x));
  const minY = Math.min(...laid.map((b) => b.y));
  let maxX = Math.max(...[...boxes.entries()].map(([id, b]) => b.x + b.w + laneRoom(id)));
  let maxY = Math.max(...laid.map((b) => b.y + b.h));
  // Below the lowest lane turn (one per published port of the busiest card).
  const rowY = maxY + 54 + Math.max(0, ...[...lanes.values()].map((n) => n * LANE_STEP));
  let docked = false;
  placePortTabs(topo, boxes, size, handleAt).forEach((p) => {
    boxes.set(p.e.target, { x: p.x, y: rowY, w: p.w, h: p.h });
    routes.set(p.e.id, tabRoute(p, rowY));
    // Half a tab beyond the last anchor, so the tab isn't cut at the drawing's edge.
    const tab = p.x + p.w / 2;
    minX = Math.min(minX, tab - PORT_TAB_SLOT / 2);
    maxX = Math.max(maxX, tab + PORT_TAB_SLOT / 2);
    maxY = Math.max(maxY, rowY + p.h);
    docked = true;
  });
  const bounds = { x: minX, y: minY, w: maxX - minX, h: maxY - minY, docked };
  const width = bounds.w + 48;
  const height = bounds.h + 48;
  return { boxes, routes, width, height, bounds };
}
