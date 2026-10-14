"use client";

import { useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Pencil, Trash2, X, XCircle, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { type ServerCheck, type ServerHost, type ServerTest } from "@/lib/tauri";
import { StatusPill, type Tone } from "@/components/ui/status-pill";
import { hasBudget, isOverBudget } from "@/features/cloud/cloud-model";
import { useT } from "@/lib/i18n";
import { formatUsd } from "@/lib/format";

export function AccountRow({
  host,
  test,
  spent,
  onTest,
  onEdit,
  onRemove,
}: {
  host: ServerHost;
  test: ServerTest | "testing" | undefined;
  spent?: number | null;
  onTest: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const result = test && test !== "testing" ? test : null;
  const ok = result ? result.ok : null;
  // Status dot: red on any failure, amber when something only warrants a warning, else green.
  const dot = result == null ? null : !result.ok ? "fail" : result.checks?.some((c) => c.state === "warn") ? "warn" : "ok";
  const [confirming, setConfirming] = useState(false);
  const facts = [
    host.host,
    host.provider === "digitalocean" || host.provider === "linode"
      ? t("cloud.account.apiToken")
      : host.provider === "oci"
        ? t("cloud.account.apiKey")
        : host.provider !== "aws"
          ? t("cloud.account.cliSignIn")
          : host.useCliCreds
            ? host.awsProfile
              ? t("cloud.account.cliProfile", { profile: host.awsProfile })
              : t("cloud.account.cliCredentials")
            : t("cloud.account.accessKeys"),
    host.autoStopHours ? t("cloud.account.autoStop", { hours: host.autoStopHours }) : t("cloud.account.noAutoStop"),
  ];
  const tone: Tone = test === "testing" ? "muted" : dot === null ? "muted" : dot;
  const label = t(
    test === "testing"
      ? "cloud.account.status.testing"
      : dot === null
        ? "cloud.account.status.notTested"
        : dot === "ok"
          ? "cloud.account.status.connected"
          : dot === "warn"
            ? "cloud.account.status.needsAttention"
            : "cloud.account.status.failed",
  );
  const isOver = isOverBudget(host, spent);
  return (
    <div className="border-t border-border first:border-t-0">
      <div className="grid min-h-[3.25rem] grid-cols-[2.25rem_minmax(0,1fr)_auto] items-center gap-3.5 px-4 py-2.5 transition-colors hover:bg-glass">
        <LogoTile provider={host.provider} />
        <div className="min-w-0">
          <p className="truncate text-[0.8125rem] font-medium text-foreground">{host.name}</p>
          <p className="truncate font-mono text-[0.6875rem] text-faint">
            {facts.join(" · ")}
            {result?.latencyMs != null ? ` · ${result.latencyMs} ms` : ""}
            {hasBudget(host) ? ` · ${t("cloud.account.budget", { amount: formatUsd(host.monthlyLimit!, t.locale, { whole: true }) })}` : ""}
            {hasBudget(host) && spent != null ? `, ${t("cloud.account.spentThisMonth", { amount: formatUsd(spent, t.locale) })}` : ""}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {confirming ? (
            <>
              <span className="text-[0.75rem] text-muted-foreground">{t("cloud.account.removeConfirm")}</span>
              <Button variant="destructive" size="xs" onClick={onRemove}>
                {t("cloud.account.remove")}
              </Button>
              <IconButton label={t("cloud.account.cancel")} onClick={() => setConfirming(false)}>
                <X className="size-3.5" />
              </IconButton>
            </>
          ) : (
            <>
              <StatusPill tone={isOver ? "fail" : tone} pulse={test === "testing"}>
                {isOver ? t("cloud.account.status.overBudget") : label}
              </StatusPill>
              <Button variant="outline" size="xs" onClick={onTest} disabled={test === "testing"}>
                {test === "testing" ? <Spinner className="size-3" /> : <Zap className="size-3" />} {t("cloud.account.test")}
              </Button>
              <IconButton label={t("cloud.account.edit")} onClick={onEdit}>
                <Pencil className="size-3.5" />
              </IconButton>
              <IconButton label={t("cloud.account.remove")} onClick={() => setConfirming(true)}>
                <Trash2 className="size-3.5" />
              </IconButton>
            </>
          )}
        </div>
      </div>
      {result &&
        (result.checks?.length ? (
          <ul className="space-y-1 px-4 pb-3 pl-[4.125rem]">
            {result.checks.map((c) => (
              <li key={c.name} className="flex items-start gap-2 text-[0.75rem]">
                <CheckGlyph state={c.state} />
                <span className="min-w-0 break-words">
                  <span className="font-medium text-foreground">{c.name}:</span> <span className="text-muted-foreground">{c.detail}</span>
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="flex items-start gap-2 px-4 pb-3 pl-[4.125rem] text-[0.75rem] text-muted-foreground">
            {ok ? <CheckCircle2 className="mt-px size-3.5 shrink-0 text-success" /> : <XCircle className="mt-px size-3.5 shrink-0 text-destructive" />}
            <span className="min-w-0 break-words">{result.message}</span>
          </p>
        ))}
    </div>
  );
}

/** The provider's logo in the 2.25rem glass tile that leads a provider row. */
export function LogoTile({ provider, label }: { provider: string; label?: string }) {
  return (
    <span className="grid size-9 shrink-0 place-items-center rounded-control bg-glass-2 shadow-[inset_0_0_0_1px_var(--input)]">
      {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
      <img src={`/brands/${provider}.svg`} alt={label ?? ""} className="size-5 object-contain" draggable={false} />
    </span>
  );
}

function CheckGlyph({ state }: { state: ServerCheck["state"] }) {
  if (state === "ok") return <CheckCircle2 className="mt-px size-3.5 shrink-0 text-success" />;
  if (state === "warn") return <AlertTriangle className="mt-px size-3.5 shrink-0 text-warning" />;
  return <XCircle className="mt-px size-3.5 shrink-0 text-destructive" />;
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-glass-2 hover:text-foreground"
    >
      {children}
    </button>
  );
}
