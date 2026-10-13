"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Laptop, Plug, Server } from "lucide-react";
import type { LabNetwork } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useLocale, useT } from "@/lib/i18n";
import { Flow, type Tab } from "@/features/labs/network-diagram/flow";
import { Attacker, MIN_ZOOM, Machine } from "@/features/labs/network-diagram/model";
import { CopyText } from "@/features/labs/network-diagram/nodes";
import { topology } from "@/features/labs/network-diagram/topology";

// The lab network diagram. Facts come from Docker (networks, interfaces, ports) or the lab's
// declarations (services); ELK lays them out (topology.ts -> layout.ts), React Flow draws them
// (flow.tsx, nodes.tsx, edges.tsx). Each network is a zone with its bridge on the top edge, uplinked
// under the card header when it has a route out; machines on one network sit in its zone, a
// machine on several (a pivot) sits between them in attack order. The attack box is a host on
// the network it joined. Published ports are tabs on the card's bottom edge (the host's wall).

/** What the diagram draws, as one string: it is laid out again only when this changes (not on
 *  every status poll). The labels are drawn in the current language: a language change re-lays
 *  them out too. */
export function topologySig(locale: string, machines: Machine[], networks: LabNetwork[], attacker: Attacker) {
  return (
    `${locale}#` +
    machines
      .map(
        (m) =>
          `${m.name}:${m.state}:${m.ip}:${(m.interfaces ?? []).map((i) => `${i.network}=${i.ip}`).join(",")}:${(m.services ?? []).map((v) => `${v.name}=${v.kind}/${v.ports.join("+")}`).join(",")}:${m.ports.map((p) => `${p.published}-${p.target}`).join(",")}`,
      )
      .join("|") +
    `#${networks.map((n) => `${n.name}=${n.subnet}${n.internal ? "!" : ""}`).join(",")}` +
    `#${attacker?.running ? `${attacker.ip}@${attacker.labNetwork ?? ""}` : "off"}`
  );
}

/** The shell's size for the graph (`graph`, the drawing's size plus its vertical margins; the
 *  side margins are 24px each) at the shell's width. A wide lab is never shrunk below a readable
 *  zoom: its canvas grows past the card instead, and the card scrolls sideways (trackpad,
 *  shift+wheel, scrollbar). Tall labs get room instead of being shrunk to fit. */
export function shellSize(graph: { w: number; h: number } | null, width: number) {
  const fit = graph && width ? Math.min(1.05, (width - 48) / Math.max(graph.w, 1)) : 1;
  const scale = Math.max(fit, MIN_ZOOM);
  const canvas = graph && width ? Math.max(width, Math.ceil(graph.w * scale + 48)) : width;
  const height = graph ? Math.round(Math.min(720, Math.max(220, graph.h * scale))) : 430;
  return { canvas, height };
}

/** The shell's width, tracked so a resized window re-fits. */
function useShellWidth() {
  const shell = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = shell.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(Math.round(e.contentRect.width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [shell, width] as const;
}

/** The card's header: your machine (127.0.0.1) or the server the lab runs on. */
function HostHeader({ host }: { host: string | null }) {
  const t = useT();
  return (
    <div className="hostcard-header">
      <span className="hostcard-icon">{host ? <Server size={16} /> : <Laptop size={16} />}</span>
      <div>
        <div className="hostcard-title">{host ?? t("diagram.host.yourMachine")}</div>
        <div className="hostcard-sub mono">
          {host ? (
            t("diagram.host.serverSub")
          ) : (
            <>
              <CopyText text="127.0.0.1">127.0.0.1</CopyText> · {t("diagram.host.host")}
            </>
          )}
        </div>
      </div>
      <span className="hostcard-online">
        <i /> {t("diagram.host.online")}
      </span>
    </div>
  );
}

/** The published ports as tabs on the card's bottom edge, following the sideways scroll. */
function PortTabs({ tabs, scrollX }: { tabs: Tab[]; scrollX: number }) {
  const t = useT();
  return (
    <div className="port-tabs">
      {tabs.map((tab) => (
        <span key={tab.id} className="port-tab" style={{ left: tab.left - scrollX }} title={t("diagram.host.portTab", { url: `http://127.0.0.1:${tab.port}` })}>
          <CopyText text={`http://127.0.0.1:${tab.port}`}>
            <Plug size={11} />
            <span className="mono">:{tab.port}</span>
          </CopyText>
        </span>
      ))}
    </div>
  );
}

export function NetworkDiagram({
  machines,
  networks = [],
  attacker = null,
  host = null,
}: {
  machines: Machine[];
  networks?: LabNetwork[];
  attacker?: Attacker;
  host?: string | null;
}) {
  const sig = topologySig(useLocale(), machines, networks, attacker);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- sig captures everything drawn
  const topo = useMemo(() => topology(machines, networks, attacker), [sig]);
  const [shell, width] = useShellWidth();
  const [graph, setGraph] = useState<{ w: number; h: number } | null>(null);
  const onSize = useCallback((w: number, h: number) => setGraph({ w, h }), []);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [scrollX, setScrollX] = useState(0);
  const { canvas, height } = shellSize(graph, width);
  // A wider window can leave less (or nothing) to scroll: the browser clamps the scroller's
  // offset without always firing a scroll event, so read it back, or the fades and the port tabs
  // would keep the old offset.
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scroller.current) setScrollX(scroller.current.scrollLeft);
  }, [canvas, width]);

  return (
    <div className={cn("hostcard", machines.some((m) => m.ports.some((p) => p.published > 0)) && "hostcard-docked")}>
      <HostHeader host={host} />
      <div
        ref={shell}
        className="topology-shell"
        style={{ height }}
        // Fades on the sides that have more to scroll to, so a wide lab reads as scrollable.
        data-more-left={scrollX > 1 || undefined}
        data-more-right={canvas - width - scrollX > 1 || undefined}
      >
        <div ref={scroller} className="topology-scroll" onScroll={(e) => setScrollX(e.currentTarget.scrollLeft)}>
          <div className="topology-canvas" style={{ width: canvas || "100%" }}>
            <ReactFlowProvider key={sig}>
              <Flow topo={topo} onSize={onSize} onTabs={setTabs} width={canvas} height={height} />
            </ReactFlowProvider>
          </div>
        </div>
        <PortTabs tabs={tabs} scrollX={scrollX} />
      </div>
    </div>
  );
}
