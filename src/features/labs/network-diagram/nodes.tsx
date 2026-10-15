"use client";

import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { DoorOpen, Lock, Monitor, Network, ShieldCheck, Terminal } from "lucide-react";
import { cn } from "@/lib/utils";
import { ComputerData, ServiceRow, ZoneData, attack, isIp, netLabel, publishHandle, serviceMeta } from "@/features/labs/network-diagram/model";
import { useT, type T } from "@/lib/i18n";
import { CopyText } from "@/features/labs/network-diagram/copy-text";

// React Flow node cards for the lab network diagram.

function PortHandles({ accent = "var(--faint)", sides = false, top = true }: { accent?: string; sides?: boolean; top?: boolean }) {
  const style = { "--handle-accent": accent } as React.CSSProperties;
  return (
    <>
      {top && <Handle type="target" position={Position.Top} className="topology-handle" style={style} />}
      <Handle type="source" position={Position.Bottom} className="topology-handle" style={style} />
      {/* A pivot links to whole network segments, left (nearer) and right (deeper). */}
      {sides && <Handle id="w" type="target" position={Position.Left} className="topology-handle" style={style} />}
      {sides && <Handle id="e" type="source" position={Position.Right} className="topology-handle" style={style} />}
    </>
  );
}

/** Where a published port's link ends: an invisible anchor on the card's bottom edge. The
 *  visible tab is drawn over the edge by the diagram (outside the canvas, which clips). */
function HostPortNode() {
  return (
    <div className="hostport-anchor">
      <Handle type="target" position={Position.Top} className="topology-handle hostport-handle" />
    </div>
  );
}

/** Where a bridge's uplink starts: an invisible point far above, hidden by the card header. */
function UplinkNode() {
  return (
    <div className="hostport-anchor">
      <Handle type="source" position={Position.Bottom} className="topology-handle hostport-handle" />
    </div>
  );
}

function ZoneNode({ data }: NodeProps<Node<ZoneData>>) {
  const t = useT();
  return (
    <div className={cn("zone", data.tone === "attack" && "zone-attack")}>
      {/* The segment's own ports: pivots plug into the network here. */}
      <Handle
        id="w"
        type="target"
        position={Position.Left}
        className="topology-handle zone-handle"
        style={data.portW !== undefined ? { top: data.portW } : undefined}
      />
      <Handle
        id="e"
        type="source"
        position={Position.Right}
        className="topology-handle zone-handle"
        style={data.portE !== undefined ? { top: data.portE } : undefined}
      />
      <span className="zone-label" style={data.labelLeft !== undefined ? { left: data.labelLeft } : undefined}>
        <span className="zone-title">
          {data.label}
          {data.isolated && (
            <span className="zone-isolated">
              <Lock size={9} /> {t("diagram.zone.noInternet")}
            </span>
          )}
        </span>
        {data.detail && <span className="zone-detail mono">{data.detail}</span>}
      </span>
    </div>
  );
}

/** A network bridge (the Docker bridge = a switch). Machines plug into it. */
function BridgeNode({ data }: NodeProps<Node<{ label: string; tone?: "attack" }>>) {
  return (
    <div className={`bridge ${data.tone === "attack" ? "bridge-attack" : ""}`}>
      <PortHandles accent={data.tone === "attack" ? attack : "var(--faint)"} top={false} />
      {/* The uplink to the host arrives here, unseen: the line just leaves the bridge upward. */}
      <Handle id="up" type="target" position={Position.Top} className="topology-handle hostport-handle" />
      <Network size={16} />
      <span className="bridge-label">{data.label}</span>
    </div>
  );
}

function AttackerNode({ data }: NodeProps<Node<{ label: string; subtitle: string; running: boolean }>>) {
  const t = useT();
  return (
    <div className="topology-node attacker-node">
      <PortHandles accent={attack} />
      <div className="attacker-icon">
        <Terminal size={18} />
      </div>
      <div className="node-copy">
        <div className="attacker-kicker">
          <span className="you-chip">{t("diagram.attacker.you")}</span> {t("diagram.attacker.kicker")}
        </div>
        <div className="node-title">{data.label}</div>
        <div className="node-subtitle" style={{ display: "flex", alignItems: "center", gap: "0.375rem" }}>
          <span className="attacker-state" data-on={data.running} />
          {data.running && isIp(data.subtitle) ? <CopyText text={data.subtitle}>{data.subtitle}</CopyText> : data.subtitle}
        </div>
      </div>
      <ShieldCheck className="attacker-shield" size={17} />
    </div>
  );
}

/** One service row: what it is, and the ports it listens on (inside the container). Publishing
 *  to the host is shown once, by the 127.0.0.1 node below, not here too. */
function ServiceChip({ row, ip }: { row: ServiceRow; ip: string }) {
  const t = useT();
  const meta = serviceMeta[row.type];
  const ServiceIcon = meta.icon;
  // Where this service's published ports leave the card: the right edge of its box, spread
  // down it when it publishes more than one.
  const published = row.ports.filter((p) => p.published > 0);
  return (
    <div className="service-chip" style={{ "--service-color": meta.color } as React.CSSProperties}>
      {published.map((p, i) => (
        <Handle
          key={p.published}
          id={publishHandle(p.published)}
          type="source"
          position={Position.Right}
          className="topology-handle publish-handle"
          style={{ top: `${String(((i + 1) * 100) / (published.length + 1))}%` }}
        />
      ))}
      <div className="service-icon">
        <ServiceIcon size={15} />
      </div>
      <div className="service-main">
        <span>{row.title}</span>
        <small>{row.label}</small>
      </div>
      <div className="service-ports">
        {row.ports.length === 0 ? (
          <span className="door-none">{t("diagram.computer.noPorts")}</span>
        ) : (
          row.ports.map((p, i) => {
            const bind = p.target || p.published;
            return (
              <CopyText key={i} text={`${ip}:${String(bind)}`}>
                <span className="door">
                  <DoorOpen size={12} />
                  <span className="mono">:{bind}</span>
                </span>
              </CopyText>
            );
          })
        )}
      </div>
    </div>
  );
}

function ComputerNode({ data }: NodeProps<Node<ComputerData>>) {
  const t = useT();
  const meta = serviceMeta[data.rows[0]?.type ?? data.type];
  const ip = data.ifaces[0]?.ip ?? "";
  const pivot = data.ifaces.length > 1;
  return (
    <div className="topology-node computer-node">
      <PortHandles accent={meta.color} sides={pivot} />
      <div className="computer-header">
        <div className="computer-name">
          <Monitor size={15} />
          <span className="mono">{data.hostname}</span>
        </div>
        <span className="running">
          <i /> {stateLabel(t, data.state, data.running)}
        </span>
      </div>
      {pivot ? (
        // One row per network: a pivot is reachable at a different address on each.
        <div className="ifaces">
          {data.ifaces.map((i) => (
            <div key={i.network} className="iface">
              <span className="iface-net">{netLabel(i.network)}</span>
              <span className="mono">{isIp(i.ip) ? <CopyText text={i.ip}>{i.ip}</CopyText> : i.ip}</span>
            </div>
          ))}
        </div>
      ) : (
        <div className="computer-ip mono">{isIp(ip) ? <CopyText text={ip}>{ip}</CopyText> : ip || t("diagram.computer.resolving")}</div>
      )}
      {/* With declared services, the image moves up here: the rows below name the services. */}
      {data.rows.length > 1 || data.rows[0]?.title !== data.image ? <div className="computer-image mono">{data.image}</div> : null}
      <div className="service-rows">
        {data.rows.map((r) => (
          <ServiceChip key={r.title} row={r} ip={ip} />
        ))}
      </div>
    </div>
  );
}

export const nodeTypes = { zone: ZoneNode, bridge: BridgeNode, attacker: AttackerNode, computer: ComputerNode, hostport: HostPortNode, uplink: UplinkNode };

/** A machine's state as a word: Vagrant's `saved`/`paused` and `poweroff`, Docker's `exited`. */
function stateLabel(t: T, state: string, running: boolean): string {
  if (running) return t("diagram.states.running");
  if (state === "saved" || state === "paused") return t("diagram.states.paused");
  if (state === "poweroff" || state === "exited" || state === "stopped") return t("diagram.states.off");
  return state || t("diagram.states.stopped");
}
