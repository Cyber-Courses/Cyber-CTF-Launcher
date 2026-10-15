"use client";

import { FlaskConical, Pencil, Star, Trash2, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { KIND } from "@/features/servers/host-setup";
import { type HostCapacity, type ServerHost } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { ServerSelfTest } from "@/features/servers/server-self-test";
import { StatusPill } from "@/components/ui/status-pill";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HostMenu } from "@/features/servers/host-menu";
import { HostExtras, LocalNetworkHint } from "@/features/servers/host-extras";
import { hostStatus, isPrivateHost, type HostTest } from "@/features/servers/servers-model";
import { formatBytes } from "@/lib/format";
import type { VmTest } from "@/features/servers/vm-tests";
import { isMac } from "@/lib/platform";
import { useT, type T } from "@/lib/i18n";

/** What a host row shows beyond the host's saved settings. */
export type HostView = {
  isDefault: boolean;
  /** The labs running on it, as last read. */
  running: number;
  capacity: HostCapacity | null | undefined;
  test: HostTest | "testing" | undefined;
  lastVm: VmTest | undefined;
  /** Whether its VM test is open under the row. */
  vmTesting: boolean;
};

/** What a host row can do, each bound to its host. */
export type HostActions = {
  onTest: () => void;
  onVmTest: () => void;
  onVmTestDone: (result: "ok" | "fail") => void;
  onEdit: () => void;
  onDefault: () => void;
  onRemove: () => void;
};

type Status = ReturnType<typeof hostStatus>;

export function HostRow({ host, view, now, actions }: { host: ServerHost; view: HostView; now: number; actions: HostActions }) {
  const status = hostStatus(view.test);
  return (
    <div className="border-t border-border first:border-t-0">
      <div className="grid min-h-[3.25rem] grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3.5 px-4 py-2.5 transition-colors hover:bg-glass">
        <ProviderGlyph provider={host.provider} />
        <HostSummary host={host} view={view} now={now} status={status} onDefault={actions.onDefault} />
        <HostControls view={view} status={status} actions={actions} />
      </div>
      {view.vmTesting && (
        <div className="px-4 pb-3.5">
          <ServerSelfTest id={host.id} provider={host.provider} onDone={actions.onVmTestDone} />
        </div>
      )}
    </div>
  );
}

/** The host's kind, address, node and capacity, as the row's facts line lists them. */
function hostFacts(t: T, host: ServerHost, capacity: HostCapacity | null | undefined): string[] {
  const facts = [KIND[host.provider].label, `${host.username}@${host.host}:${String(host.port)}`];
  if (host.node) facts.push(t("servers.host.node", { node: host.node }));
  if (capacity) {
    facts.push(t("servers.screen.vcpu", { count: capacity.cores }));
    facts.push(t("servers.host.memFree", { free: formatBytes(capacity.memFree), total: formatBytes(capacity.memTotal) }));
  }
  return facts;
}

/** The name and default toggle, the facts, then what is known of the host's labs and tests. */
function HostSummary({ host, view, now, status, onDefault }: { host: ServerHost; view: HostView; now: number; status: Status; onDefault: () => void }) {
  const t = useT();
  const { result, needsPassword, online } = status;
  const failed = result !== null && !result.ok;
  return (
    <div className="min-w-0">
      <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.8125rem] font-medium text-foreground">
        <span className="truncate">{host.name}</span>
        <DefaultToggle isDefault={view.isDefault} onClick={onDefault} />
      </p>
      {/* Wraps between facts (never inside one): CPU and memory stay readable in a narrow
          window or a longer language instead of being cut off. */}
      <p className="mt-0.5 flex flex-wrap gap-x-1.5 font-mono text-[0.6875rem] text-faint">
        {hostFacts(t, host, view.capacity).map((f, i) => (
          <span key={f} className="max-w-full truncate whitespace-nowrap">
            {i > 0 && <span aria-hidden>· </span>}
            {f}
          </span>
        ))}
      </p>
      {/* The running-labs count is filesystem-only and can go stale when a host drops. Only
          trust it once the host answers as Online; an unreachable host must not report a
          phantom count. */}
      <HostExtras running={online ? view.running : 0} lastVm={view.lastVm} now={now} failure={failed ? result.message : null} needsPassword={needsPassword} />
      {/* A denied Local Network permission looks exactly like a host that is down. */}
      {failed && !needsPassword && isMac() && isPrivateHost(host.host) && <LocalNetworkHint />}
    </div>
  );
}

/** The status pill, Test, and the overflow menu (VM test, edit, remove). */
function HostControls({ view, status, actions }: { view: HostView; status: Status; actions: HostActions }) {
  const t = useT();
  const testing = view.test === "testing";
  const latency = status.result?.latencyMs ?? null;
  return (
    <div className="flex shrink-0 items-center gap-2">
      <StatusPill tone={status.tone} pulse={testing}>
        {t(`servers.host.status.${status.status}`)}
        {latency !== null && <span className="font-mono text-[0.6875rem] font-normal tabular-nums text-faint">{latency} ms</span>}
      </StatusPill>
      <Button variant="outline" size="xs" onClick={actions.onTest} disabled={testing}>
        {testing ? <Spinner className="size-3" /> : <Zap className="size-3" />} {t("servers.host.test")}
      </Button>
      <HostMenu
        items={[
          { label: view.vmTesting ? t("servers.host.hideVmTest") : t("servers.host.vmTest"), icon: FlaskConical, onClick: actions.onVmTest },
          { label: t("servers.host.edit"), icon: Pencil, onClick: actions.onEdit },
          { label: t("servers.host.remove"), icon: Trash2, danger: true, onClick: actions.onRemove },
        ]}
      />
    </div>
  );
}

/** Beside the host's name: a filled "Default" chip on the default host (pressed: a click
 *  unsets it), a quiet "Set as default" button on the others. */
function DefaultToggle({ isDefault, onClick }: { isDefault: boolean; onClick: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={isDefault}
      title={isDefault ? t("servers.host.isDefaultTitle") : t("servers.host.makeDefaultTitle")}
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-px font-mono text-[0.625rem] font-medium whitespace-nowrap transition-colors",
        isDefault
          ? "bg-jewel/[0.12] text-jewel-text shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--jewel)_40%,transparent)]"
          : "text-faint hover:text-foreground hover:shadow-[inset_0_0_0_1px_var(--input)]",
      )}
    >
      <Star className={cn("size-2.5", isDefault && "fill-current")} /> {isDefault ? t("servers.host.default") : t("servers.host.setDefault")}
    </button>
  );
}
