"use client";

import { BaseEdge, EdgeLabelRenderer, getSmoothStepPath, type Edge, type EdgeProps } from "@xyflow/react";
import { GREY, LinkData, Pt } from "@/features/labs/network-diagram/model";

// Links between diagram nodes: ELK's orthogonal routes, drawn with rounded corners.

/** A point as SVG path coordinates. */
const xy = (p: Pt) => `${String(p.x)} ${String(p.y)}`;

/** An orthogonal polyline with rounded corners (the network-diagram look). */
function roundedPath(pts: Pt[], r = 8) {
  if (pts.length < 2) return "";
  let d = `M ${xy(pts[0])}`;
  for (let i = 1; i < pts.length - 1; i++) {
    const [a, b, c] = [pts[i - 1], pts[i], pts[i + 1]];
    const rad = Math.min(r, Math.hypot(b.x - a.x, b.y - a.y) / 2, Math.hypot(c.x - b.x, c.y - b.y) / 2);
    const into = { x: b.x - Math.sign(b.x - a.x) * rad, y: b.y - Math.sign(b.y - a.y) * rad };
    const out = { x: b.x + Math.sign(c.x - b.x) * rad, y: b.y + Math.sign(c.y - b.y) * rad };
    d += ` L ${xy(into)} Q ${xy(b)} ${xy(out)}`;
  }
  const last = pts[pts.length - 1];
  return `${d} L ${xy(last)}`;
}

/** Midpoint of the longest segment: where a label sits without hiding a corner. */
function labelPoint(pts: Pt[]): Pt {
  let best = { len: -1, at: pts[0] };
  for (let i = 1; i < pts.length; i++) {
    const len = Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
    if (len > best.len) best = { len, at: { x: (pts[i].x + pts[i - 1].x) / 2, y: (pts[i].y + pts[i - 1].y) / 2 } };
  }
  return best.at;
}

/** Draws ELK's route, as a plain cable (no arrowheads: links have no direction). If a node was dragged away from it, falls back to a smooth step. */
function LinkEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, style, data }: EdgeProps<Edge<LinkData>>) {
  const route = data?.route;
  // ELK ends on the border; the handle dot straddles it, a few px off along the link.
  const near = (a: Pt, x: number, y: number) => Math.abs(a.x - x) < 10 && Math.abs(a.y - y) < 10;
  let path: string;
  let at: Pt;
  if (route && route.length >= 2 && near(route[0], sourceX, sourceY) && near(route[route.length - 1], targetX, targetY)) {
    // Snap the ends onto the handles, dragging the neighbouring bend along so the first and
    // last segments stay straight (a few px of drift would otherwise kink into an S).
    const pts = route.map((p) => ({ ...p }));
    const snap = (end: number, next: number, x: number, y: number) => {
      const vertical = Math.abs(pts[end].x - pts[next].x) < 0.5;
      if (pts.length > 2) {
        if (vertical) pts[next].x = x;
        else pts[next].y = y;
      }
      pts[end] = { x, y };
    };
    snap(0, 1, sourceX, sourceY);
    snap(pts.length - 1, pts.length - 2, targetX, targetY);
    path = roundedPath(pts);
    at = labelPoint(pts);
  } else {
    const [p, x, y] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, borderRadius: 8 });
    path = p;
    at = { x, y };
  }
  return (
    <>
      <BaseEdge id={id} path={path} style={style} />
      {data?.label && (
        <EdgeLabelRenderer>
          <div className="edge-label" style={{ transform: `translate(-50%, -50%) translate(${String(at.x)}px,${String(at.y)}px)` }}>
            {data.label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
export const edgeTypes = { link: LinkEdge };
export function link(
  id: string,
  source: string,
  target: string,
  color: string,
  opts: { animated?: boolean; dashed?: boolean; label?: string; sideways?: boolean } = {},
): Edge<LinkData> {
  return {
    id,
    source,
    target,
    // Sideways = segment to pivot to segment, through the left/right handles.
    ...(opts.sideways ? { sourceHandle: "e", targetHandle: "w" } : {}),
    type: "link",
    animated: opts.animated,
    data: { label: opts.label },
    style: { stroke: color, strokeWidth: 1.8, strokeDasharray: opts.animated || opts.dashed ? "5 5" : undefined, opacity: color === GREY ? 0.8 : 1 },
  };
}
