"use client";

import { useEffect, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { CopyValue } from "@/components/ui/copy-value";
import { Panel, PanelHeader } from "@/components/ui/panel";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { labCloudPreview, type CloudPreview, type LabOutput } from "@/lib/tauri";
import { useT, type T } from "@/lib/i18n";
import { formatUsd } from "@/lib/format";

/** "about $0.02 an hour (about $0.08 until it auto-stops in 4 h)" from a lab's own estimate. */
export function costEstimate(t: T, hourlyUsd: number | null | undefined, autoStopHours?: number | null): string | null {
  if (hourlyUsd == null) return null;
  const hourly = t("labs.cloud.hourly", { usd: formatUsd(hourlyUsd, t.locale) });
  return autoStopHours ? t("labs.cloud.untilAutoStop", { hourly, usd: formatUsd(hourlyUsd * autoStopHours, t.locale), hours: autoStopHours }) : hourly;
}

/** A cloud lab's cost and inputs, from its spec at the catalogue's commit (null until read, or
 *  for a lab that isn't cloud services). */
export function useCloudPreview(labId: string, rt: LabRuntimeInfo | null): CloudPreview | null {
  const [preview, setPreview] = useState<CloudPreview | null>(null);
  const cloud = rt?.runtime === "CLOUD";
  const repository = rt?.repository;
  const commit = rt?.commit;
  useEffect(() => {
    if (!cloud || !repository || !commit) return;
    let alive = true;
    labCloudPreview(labId, repository, commit)
      .then((p) => alive && setPreview(p))
      .catch(() => alive && setPreview(null));
    return () => {
      alive = false;
    };
  }, [cloud, labId, repository, commit]);
  return preview;
}

/**
 * Before a cloud lab starts, in the launch dialog: what it costs and what it does in the
 * player's account, so the click that spends money is an informed one.
 */
export function CloudLaunchNote({ preview, autoStopHours }: { preview: CloudPreview | null; autoStopHours: number | null }) {
  const t = useT();
  const cost = costEstimate(t, preview?.hourlyUsd, autoStopHours);
  return (
    <div className="space-y-1.5 rounded-panel bg-glass px-3.5 py-3 text-[0.75rem] leading-relaxed text-muted-foreground shadow-[inset_0_0_0_1px_var(--border)]">
      <p>
        <b className="text-foreground">{t("labs.cloud.billed")}</b>{" "}
        {autoStopHours ? t("labs.cloud.costsUntilAutoStop", { hours: autoStopHours }) : t("labs.cloud.costsUntilStop")}
      </p>
      <p>{cost ? t("labs.cloud.estimate", { cost }) : t("labs.cloud.noEstimate")}</p>
      {preview?.allowList && <p>{t("labs.cloud.allowList")}</p>}
    </div>
  );
}

/** One output: copyable, a secret masked until asked. */
function OutputRow({ output }: { output: LabOutput }) {
  const t = useT();
  const [shown, setShown] = useState(!output.sensitive);
  return (
    <div className="flex items-center justify-between gap-3 border-t border-border px-4 py-2.5 text-[0.8125rem] first:border-t-0">
      <span className="shrink-0 font-mono text-[0.75rem] text-muted-foreground">{output.name}</span>
      <span className="flex min-w-0 items-center gap-1.5">
        <CopyValue text={output.value} label={shown ? undefined : "••••••••"} title={shown ? undefined : t("labs.cloud.copySecret", { name: output.name })} />
        {output.sensitive && (
          <button
            type="button"
            onClick={() => setShown((s) => !s)}
            className="text-faint transition-colors hover:text-foreground"
            aria-label={t(shown ? "labs.cloud.hide" : "labs.cloud.show", { name: output.name })}
          >
            {shown ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
          </button>
        )}
      </span>
    </div>
  );
}

/**
 * A running cloud lab: its own words (where to start) and what it exposes (its module's
 * outputs: addresses, keys). There is no network diagram: no machines, only services.
 */
export function CloudPanel({ message, outputs, where }: { message: string | null; outputs: LabOutput[]; where: string }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.cloudServicesTitle")} meta={where} />
      {message && <p className="whitespace-pre-wrap px-4 py-3.5 font-mono text-[0.75rem] leading-relaxed text-foreground">{message}</p>}
      {outputs.length > 0 && (
        <div className={message ? "border-t border-border" : undefined}>
          {outputs.map((o) => (
            <OutputRow key={o.name} output={o} />
          ))}
        </div>
      )}
      {!message && outputs.length === 0 && <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("labs.cloud.nothingExposed")}</p>}
    </Panel>
  );
}

/** A cloud lab that isn't up: a note on what a start does, or on services a cut-off start left. */
export function CloudNotePanel({ note, meta }: { note: "cloudLeftover" | "cloudIdle"; meta: string }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.cloudServicesTitle")} meta={meta} />
      <p className="dotted-canvas px-6 py-14 text-center text-[0.8125rem] text-muted-foreground">{t(`labs.detail.${note}`)}</p>
    </Panel>
  );
}
