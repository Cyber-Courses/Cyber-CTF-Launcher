"use client";

import { Check, FlaskConical, Pencil, Star, Trash2, X, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { KIND } from "@/features/servers/host-setup";
import { type HostCapacity, type ServerHost, type ServerTest } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { ServerSelfTest } from "@/features/servers/server-self-test";
import { StatusDot, StatusPill } from "@/components/ui/status-pill";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HostMenu } from "@/features/servers/host-menu";
import { hostStatus, isPrivateHost } from "@/features/servers/servers-model";
import { formatAgo, formatBytes } from "@/lib/format";
import { VmTest } from "@/features/servers/vm-tests";
import { openExternal } from "@/lib/failure";
import { isMac } from "@/lib/platform";
import { useT } from "@/lib/i18n";

const LOCAL_NETWORK_SETTINGS = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";

export function HostRow({
  host,
  isDefault,
  running,
  capacity,
  test,
  lastVm,
  now,
  vmTesting,
  onTest,
  onVmTest,
  onVmTestDone,
  onEdit,
  onDefault,
  onRemove,
}: {
  host: ServerHost;
  isDefault: boolean;
  running: number;
  capacity: HostCapacity | null | undefined;
  test: ServerTest | "testing" | undefined;
  lastVm: VmTest | undefined;
  now: number;
  vmTesting: boolean;
  onTest: () => void;
  onVmTest: () => void;
  onVmTestDone: (result: "ok" | "fail") => void;
  onEdit: () => void;
  onDefault: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const { result, needsPassword, tone, status, online } = hostStatus(test);
  const facts = [
    KIND[host.provider].label,
    `${host.username}@${host.host}:${host.port}`,
    host.node ? t("servers.host.node", { node: host.node }) : null,
    capacity ? t("servers.screen.vcpu", { count: capacity.cores }) : null,
    capacity ? t("servers.host.memFree", { free: formatBytes(capacity.memFree), total: formatBytes(capacity.memTotal) }) : null,
  ].filter(Boolean);
  return (
    <div className="border-t border-border first:border-t-0">
      <div className="grid min-h-[3.25rem] grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3.5 px-4 py-2.5 transition-colors hover:bg-glass">
        <ProviderGlyph provider={host.provider} />
        <div className="min-w-0">
          <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.8125rem] font-medium text-foreground">
            <span className="truncate">{host.name}</span>
            <DefaultToggle isDefault={isDefault} onClick={onDefault} />
          </p>
          {/* Wraps between facts (never inside one): CPU and memory stay readable in a narrow
              window or a longer language instead of being cut off. */}
          <p className="mt-0.5 flex flex-wrap gap-x-1.5 font-mono text-[0.6875rem] text-faint">
            {facts.map((f, i) => (
              <span key={i} className="max-w-full truncate whitespace-nowrap">
                {i > 0 && <span aria-hidden>· </span>}
                {f}
              </span>
            ))}
          </p>
          {/* The running-labs count is filesystem-only and can go stale when a host drops. Only
              trust it once the host answers as Online; an unreachable host must not report a
              phantom count. */}
          <HostExtras
            running={online ? running : 0}
            lastVm={lastVm}
            now={now}
            failure={result && !result.ok ? result.message : null}
            needsPassword={needsPassword}
          />
          {/* A denied Local Network permission looks exactly like a host that is down. */}
          {result && !result.ok && !needsPassword && isMac() && isPrivateHost(host.host) && <LocalNetworkHint />}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <StatusPill tone={tone} pulse={test === "testing"}>
            {t(`servers.host.status.${status}`)}
            {result?.latencyMs != null && <span className="font-mono text-[0.6875rem] font-normal tabular-nums text-faint">{result.latencyMs} ms</span>}
          </StatusPill>
          <Button variant="outline" size="xs" onClick={onTest} disabled={test === "testing"}>
            {test === "testing" ? <Spinner className="size-3" /> : <Zap className="size-3" />} {t("servers.host.test")}
          </Button>
          <HostMenu
            items={[
              { label: vmTesting ? t("servers.host.hideVmTest") : t("servers.host.vmTest"), icon: FlaskConical, onClick: onVmTest },
              { label: t("servers.host.edit"), icon: Pencil, onClick: onEdit },
              { label: t("servers.host.remove"), icon: Trash2, danger: true, onClick: onRemove },
            ]}
          />
        </div>
      </div>
      {vmTesting && (
        <div className="px-4 pb-3.5">
          <ServerSelfTest id={host.id} provider={host.provider} onDone={onVmTestDone} />
        </div>
      )}
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

/** Running labs, the last VM test and a failure: only the ones we actually know. */
function HostExtras({
  running,
  lastVm,
  now,
  failure,
  needsPassword,
}: {
  running: number;
  lastVm: VmTest | undefined;
  now: number;
  failure: string | null;
  needsPassword: boolean;
}) {
  const t = useT();
  if (running <= 0 && !lastVm && failure === null) return null;
  return (
    <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[0.75rem] text-muted-foreground">
      {running > 0 && (
        <span className="inline-flex items-center gap-1.5">
          <StatusDot tone="ok" /> {t("servers.host.labsRunning", { count: running })}
        </span>
      )}
      {lastVm && (
        <span className="inline-flex items-center gap-1">
          {lastVm.result === "ok" ? <Check className="size-3 text-success" /> : <X className="size-3 text-destructive" />}{" "}
          {lastVm.result === "ok" ? t("servers.host.vmTestPassed") : t("servers.host.vmTestFailed")}{" "}
          <span className="font-mono text-[0.6875rem] text-faint">{formatAgo(lastVm.at, now)}</span>
        </span>
      )}
      {failure !== null && (
        <span className="inline-flex min-w-0 items-start gap-1.5">
          <StatusDot tone={needsPassword ? "warn" : "fail"} className="mt-1.5" />
          <span className="min-w-0 break-words">{failure}</span>
        </span>
      )}
    </p>
  );
}

/** On macOS, how to grant the Local Network permission a private-address host needs. */
function LocalNetworkHint() {
  const t = useT();
  return (
    <p className="mt-1 text-[0.75rem] text-muted-foreground">
      {t("servers.host.localNetwork")}{" "}
      <button type="button" className="text-link underline underline-offset-2" onClick={() => openExternal(LOCAL_NETWORK_SETTINGS)}>
        {t("servers.host.openPrivacy")}
      </button>
    </p>
  );
}
