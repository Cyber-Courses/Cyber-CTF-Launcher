"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Plug } from "lucide-react";
import type { LabNetwork } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useLocale, useT } from "@/lib/i18n";
import { Flow, type Tab } from "@/features/labs/network-diagram/flow";
import { Attacker, MIN_ZOOM, Machine } from "@/features/labs/network-diagram/model";
import { CopyText } from "@/features/labs/network-diagram/copy-text";
import { HostHeader, hasPortTabs } from "@/features/labs/network-diagram/host-header";
import { FitWidthControls, fitShellSize, useFitWidth } from "@/features/labs/network-diagram/fit-width";
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
    machines.map(machineSig).join("|") +
    `#${networks.map((n) => `${n.name}=${n.subnet}${n.internal ? "!" : ""}`).join(",")}` +
    `#${attacker?.running ? `${attacker.ip}@${attacker.labNetwork ?? ""}` : "off"}`
  );
}

/** One machine's part of the signature: what its card and links draw. */
function machineSig(m: Machine): string {
  const interfaces = m.interfaces.map((i) => `${i.network}=${i.ip}`).join(",");
  const services = m.services.map((v) => `${v.name}=${v.kind}/${v.ports.join("+")}`).join(",");
  const ports = m.ports.map((p) => `${String(p.published)}-${String(p.target)}`).join(",");
  return `${m.name}:${m.state}:${m.ip}:${m.image}:${m.infra ? "infra" : ""}:${interfaces}:${services}:${ports}`;
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
    const ro = new ResizeObserver(([e]) => {
      setWidth(Math.round(e.contentRect.width));
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
    };
  }, []);
  return [shell, width] as const;
}

/** A published port's address on this machine. */
const localUrl = (port: number) => `http://127.0.0.1:${String(port)}`;

/** The published ports as tabs on the card's bottom edge, following the sideways scroll. */
function PortTabs({ tabs, scrollX }: { tabs: Tab[]; scrollX: number }) {
  const t = useT();
  return (
    <div className="port-tabs">
      {tabs.map((tab) => (
        <span key={tab.id} className="port-tab" style={{ left: tab.left - scrollX }} title={t("diagram.host.portTab", { url: localUrl(tab.port) })}>
          <CopyText text={localUrl(tab.port)}>
            <Plug size={11} />
            <span className="mono">:{tab.port}</span>
          </CopyText>
        </span>
      ))}
    </div>
  );
}

type DiagramProps = {
  machines: Machine[];
  networks?: LabNetwork[];
  attacker?: Attacker;
  host?: string | null;
};

export function NetworkDiagram({ machines, networks = [], attacker = null, host = null }: DiagramProps) {
  const sig = topologySig(useLocale(), machines, networks, attacker);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- sig captures everything drawn
  const topo = useMemo(() => topology(machines, networks, attacker), [sig]);
  const [shell, width] = useShellWidth();
  const [graph, setGraph] = useState<{ w: number; h: number } | null>(null);
  const onSize = useCallback((w: number, h: number) => {
    setGraph({ w, h });
  }, []);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [scrollX, setScrollX] = useState(0);
  // Wider than the card at a readable zoom: it scrolls, unless the player chose to fit it.
  const [fit, setFit] = useFitWidth();
  const natural = shellSize(graph, width);
  const wide = natural.canvas - width > 1;
  const { canvas, height } = fit && wide ? fitShellSize(graph, width) : natural;
  // A wider window can leave less (or nothing) to scroll: the browser clamps the scroller's
  // offset without always firing a scroll event, so read it back, or the fades and the port tabs
  // would keep the old offset.
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scroller.current) setScrollX(scroller.current.scrollLeft);
  }, [canvas, width]);

  return (
    <div className={cn("hostcard", hasPortTabs(machines) && "hostcard-docked")}>
      <HostHeader
        host={host}
        tools={
          <FitWidthControls
            wide={wide}
            fit={fit}
            onFit={setFit}
            moreRight={canvas - width - scrollX > 1}
            onScrollMore={() => scroller.current?.scrollBy({ left: Math.round(width * 0.8), behavior: "smooth" })}
          />
        }
      />
      <div
        ref={shell}
        className="topology-shell"
        style={{ height }}
        // Fades on the sides that have more to scroll to, so a wide lab reads as scrollable.
        data-more-left={scrollX > 1 || undefined}
        data-more-right={canvas - width - scrollX > 1 || undefined}
        data-fit={(fit && wide) || undefined}
      >
        <div
          ref={scroller}
          className="topology-scroll"
          onScroll={(e) => {
            setScrollX(e.currentTarget.scrollLeft);
          }}
        >
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
