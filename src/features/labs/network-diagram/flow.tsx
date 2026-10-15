"use client";

import { useEffect, useMemo, useState } from "react";
import { Background, BackgroundVariant, ReactFlow, useEdgesState, useNodesInitialized, useNodesState, useReactFlow, type Edge, type Node } from "@xyflow/react";
import { cn } from "@/lib/utils";
import { edgeTypes } from "@/features/labs/network-diagram/edges";
import { frameLayout } from "@/features/labs/network-diagram/frame";
import { layout } from "@/features/labs/network-diagram/layout";
import { ANCHOR, type Bounds } from "@/features/labs/network-diagram/model";
import { nodeTypes } from "@/features/labs/network-diagram/nodes";
import type { Topology } from "@/features/labs/network-diagram/topology";

/** The edges before the first layout: none (the layout routes them). */
const NO_EDGES: Edge[] = [];

/** A published port's tab over the card's bottom edge, `left` px from the shell's left. */
export type Tab = { id: string; port: number; left: number };

/** The view that fits the laid-out drawing into the shell. With published ports, their anchors
 *  sit exactly on the bottom edge of the card (your machine): ports in its wall. */
export function fittedView(bounds: Bounds, shellW: number, shellH: number) {
  const side = 24;
  const top = 26;
  const bottom = bounds.docked ? 0 : 24;
  const zoom = Math.min(1.05, (shellW - 2 * side) / Math.max(bounds.w, 1), (shellH - top - bottom) / Math.max(bounds.h, 1));
  const x = (shellW - bounds.w * zoom) / 2 - bounds.x * zoom;
  const y = bounds.docked ? shellH - (bounds.y + bounds.h) * zoom : (shellH - bounds.h * zoom) / 2 - bounds.y * zoom;
  return { x, y, zoom };
}

/** Once React Flow has measured the nodes (rendered invisibly first), ELK lays them out with
 *  their real sizes (a card grows with its interfaces and ports), then the zones are framed and
 *  the nodes and routed edges set. Ready once laid out (or the layout failed). */
function useElkLayout(topo: Topology, setNodes: (nodes: Node[]) => void, setEdges: (edges: Edge[]) => void, onSize: (w: number, h: number) => void) {
  const measured = useNodesInitialized();
  const { getNode, getInternalNode } = useReactFlow();
  const [ready, setReady] = useState(false);
  const [bounds, setBounds] = useState<Bounds | null>(null);
  useEffect(() => {
    if (!measured || ready) return;
    let live = true;
    const size = (id: string) => {
      const n = getNode(id);
      return { w: n?.measured?.width ?? 200, h: n?.measured?.height ?? 120 };
    };
    // Where a link leaves a node from a named handle (a published port's, on its service's
    // box): the handle's outer middle, relative to the node, as React Flow measured it.
    const handleAt = (id: string, handle: string) => {
      const b = getInternalNode(id)?.internals.handleBounds?.source?.find((h) => h.id === handle);
      return b ? { x: b.x + b.width, y: b.y + b.height / 2 } : undefined;
    };
    layout(topo, size, handleAt)
      .then(({ boxes, routes, bounds: laid }) => {
        if (!live) return;
        const { zones, bounds: final } = frameLayout(topo, boxes, routes, laid);
        setNodes([...zones, ...topo.nodes.map((n) => ({ ...n, position: { x: boxes.get(n.id)?.x ?? 0, y: boxes.get(n.id)?.y ?? 0 } }))]);
        setEdges(topo.edges.map((e) => ({ ...e, data: { ...e.data, route: routes.get(e.id) } })));
        setBounds(final);
        onSize(final.w, final.h + (final.docked ? 26 : 50));
        setReady(true);
      })
      .catch((e: unknown) => {
        console.warn("network diagram layout failed", e);
        if (live) setReady(true);
      });
    return () => {
      live = false;
    };
  }, [measured, ready, topo, getNode, getInternalNode, setNodes, setEdges, onSize]);
  return { ready, bounds };
}

/** The diagram itself: measured, laid out by ELK, framed, then fitted into the shell. */
export function Flow({
  topo,
  onSize,
  onTabs,
  width: shellW,
  height: shellH,
}: {
  topo: Topology;
  onSize: (w: number, h: number) => void;
  onTabs: (tabs: Tab[]) => void;
  width: number;
  height: number;
}) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>(topo.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(NO_EDGES);
  const { setViewport } = useReactFlow();
  const { ready, bounds } = useElkLayout(topo, setNodes, setEdges, onSize);

  // Fit once laid out, and again whenever the shell changes size.
  const view = useMemo(() => (!ready || !bounds || !shellW || !shellH ? null : fittedView(bounds, shellW, shellH)), [ready, bounds, shellW, shellH]);
  useEffect(() => {
    if (view) void setViewport(view);
  }, [view, setViewport]);

  // Published ports as tabs over the card's bottom edge, each under its link's anchor. The card
  // draws them, outside the scroller, so they can hang over its bottom edge.
  useEffect(() => {
    onTabs(
      ready && view
        ? nodes
            .filter((n) => n.type === "hostport")
            .map((n) => ({ id: n.id, port: (n.data as { port: number }).port, left: (n.position.x + ANCHOR / 2) * view.zoom + view.x }))
        : [],
    );
  }, [ready, view, nodes, onTabs]);

  return (
    <ReactFlow
      className={cn("transition-opacity duration-200", ready ? "opacity-100" : "opacity-0")}
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      minZoom={0.25}
      maxZoom={1.5}
      // A fitted picture, not a canvas: panning or zooming would slide the ports off the edge.
      panOnDrag={false}
      zoomOnScroll={false}
      zoomOnPinch={false}
      zoomOnDoubleClick={false}
      panOnScroll={false}
      preventScrolling={false}
      nodesConnectable={false}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="var(--input)" />
    </ReactFlow>
  );
}
