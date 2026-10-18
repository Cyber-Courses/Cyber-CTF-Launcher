import type { Node } from "@xyflow/react";
import { ANCHOR, type Bounds, type Pt } from "@/features/labs/network-diagram/model";
import type { Topology } from "@/features/labs/network-diagram/topology";

type Box = { x: number; y: number; w: number; h: number };

/** A zone's label, at the left end of its top edge (topology.css `.zone-label`): `left` px in
 *  from the frame's left, from `top` px above its top edge, `h` px tall (two lines). */
export const ZONE_LABEL = { left: 14, top: -15, h: 34 };

/** The label's width: the name in 9px caps (≈ 6.4px a character, plus the "no internet" tag),
 *  the detail (subnet) in 9.5px mono (≈ 5.8px), and its padding. */
export function zoneLabelWidth(data: Topology["zones"][number]["data"]) {
  return Math.max(data.label.length * 6.4 + (data.isolated ? 92 : 0), (data.detail ?? "").length * 5.8) + 22;
}

/** Room kept between the label and the bridge, and between the bridge and the frame's right edge. */
const LABEL_GAP = 12;

/** The narrowest a segment can be drawn: the label, then its bridge clear of it on the same top
 *  edge. Only the bridge shares the label's band (the cards sit below it), so this is all the
 *  room the label needs. The layout reserves it, so a segment never reaches over its neighbours. */
export function zoneMinWidth(data: Topology["zones"][number]["data"], bridgeW: number) {
  return ZONE_LABEL.left + zoneLabelWidth(data) + LABEL_GAP + bridgeW + LABEL_GAP;
}

/**
 * ELK's layout (layout.ts) as drawn: each network a zone framed around its members, with its
 * bridge centred on the top edge and its links re-drawn as a bus from there; uplinks straight up
 * from the bridges; and the bounds of the final shapes. `boxes` and `routes` are adjusted in place.
 */
export function frameLayout(topo: Topology, boxes: Map<string, Box>, routes: Map<string, Pt[]>, laid: Bounds): { zones: Node[]; bounds: Bounds } {
  // ELK lays out every zone; one it didn't place has nothing to frame.
  const zones: Node[] = topo.zones.flatMap((z) => {
    const b = boxes.get(z.id);
    if (!b) return [];
    const br = boxes.get(z.members[0]);
    // ELK already sized the segment for its label (zoneMinWidth); this only guards a layout that
    // came out narrower, widening evenly so its contents stay centred.
    const w = Math.max(b.w, zoneMinWidth(z.data, br?.w ?? 0));
    const x = b.x - (w - b.w) / 2;
    // The bridge sits on the segment's top edge, like a gateway: in the middle, or moved right
    // until it clears the label. The frame starts at the bridge's centre line.
    const top = br ? br.y + br.h / 2 : b.y;
    if (br) {
      br.x = Math.max(x + w / 2 - br.w / 2, x + ZONE_LABEL.left + zoneLabelWidth(z.data) + LABEL_GAP);
      // Re-draw the bridge's links as a bus from its new centre: down, across, down.
      const sx = br.x + br.w / 2;
      const sy = br.y + br.h;
      topo.edges
        .filter((e) => e.source === z.members[0])
        .forEach((e) => {
          const r = routes.get(e.id);
          if (!r) return;
          const end = r[r.length - 1];
          const busY = Math.min(sy + 20, end.y - 12);
          routes.set(e.id, Math.abs(end.x - sx) < 1 ? [{ x: sx, y: sy }, end] : [{ x: sx, y: sy }, { x: sx, y: busY }, { x: end.x, y: busY }, end]);
        });
    }
    // Side ports stay where ELK routed the pivot links (moved out with a widened frame).
    const side = (h: "w" | "e") => {
      const e = topo.edges.find((e) => (h === "e" ? e.source === z.id && e.sourceHandle === "e" : e.target === z.id && e.targetHandle === "w"));
      const r = e && routes.get(e.id);
      if (!r) return undefined;
      const pt = h === "e" ? r[0] : r[r.length - 1];
      pt.x = h === "e" ? x + w : x;
      return pt.y - top;
    };
    return {
      id: z.id,
      type: "zone" as const,
      position: { x, y: top },
      style: { width: w, height: b.y + b.h - top },
      data: { ...z.data, labelLeft: ZONE_LABEL.left, portW: side("w"), portE: side("e") },
      draggable: false,
      selectable: false,
    };
  });
  // Uplinks: straight up from each (centred) bridge to a point far above the view, so the line
  // slides under the card header without showing where it lands.
  topo.edges
    .filter((e) => e.source.startsWith("up-"))
    .forEach((e) => {
      const br = boxes.get(e.target);
      if (!br) return;
      const cx = br.x + br.w / 2;
      boxes.set(e.source, { x: cx - ANCHOR / 2, y: br.y - 2000, w: ANCHOR, h: ANCHOR });
      routes.set(e.id, [
        { x: cx, y: br.y - 2000 + ANCHOR },
        { x: cx, y: br.y },
      ]);
    });
  // Widened zones can reach past what ELK laid out: frame the final shapes.
  const x0 = Math.min(laid.x, ...zones.map((z) => z.position.x));
  const x1 = Math.max(laid.x + laid.w, ...zones.map((z) => z.position.x + Number(z.style?.width ?? 0)));
  return { zones, bounds: { ...laid, x: x0, w: x1 - x0 } };
}
