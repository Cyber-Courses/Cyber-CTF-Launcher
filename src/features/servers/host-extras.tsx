"use client";

import { Check, X } from "lucide-react";
import { StatusDot } from "@/components/ui/status-pill";
import type { VmTest } from "@/features/servers/vm-tests";
import { formatAgo } from "@/lib/format";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

const LOCAL_NETWORK_SETTINGS = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";

/** Running labs, the last VM test and a failure: only the ones we actually know. */
export function HostExtras({
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
export function LocalNetworkHint() {
  const t = useT();
  return (
    <p className="mt-1 text-[0.75rem] text-muted-foreground">
      {t("servers.host.localNetwork")}{" "}
      <button
        type="button"
        className="text-link underline underline-offset-2"
        onClick={() => {
          openExternal(LOCAL_NETWORK_SETTINGS);
        }}
      >
        {t("servers.host.openPrivacy")}
      </button>
    </p>
  );
}
