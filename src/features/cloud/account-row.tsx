"use client";

import { useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Pencil, Trash2, X, XCircle, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { type ServerCheck, type ServerHost, type ServerTest } from "@/lib/tauri";
import { StatusPill } from "@/components/ui/status-pill";
import { budgetOf, credentialOf, isOverBudget, outcomeOf, type Credential, type Outcome } from "@/features/cloud/cloud-model";
import { useT, type T } from "@/lib/i18n";
import { formatUsd } from "@/lib/format";

/** An account's test: running, its result, or never run. */
type TestState = ServerTest | "testing" | undefined;

const OUTCOME_LABEL = {
  ok: "cloud.account.status.connected",
  warn: "cloud.account.status.needsAttention",
  fail: "cloud.account.status.failed",
} as const satisfies Record<Outcome, string>;

export function AccountRow({
  host,
  test,
  spent,
  onTest,
  onEdit,
  onRemove,
}: {
  host: ServerHost;
  test: TestState;
  spent?: number | null;
  onTest: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const result = test === "testing" ? undefined : test;
  return (
    <div className="border-t border-border first:border-t-0">
      <div className="grid min-h-[3.25rem] grid-cols-[2.25rem_minmax(0,1fr)_auto] items-center gap-3.5 px-4 py-2.5 transition-colors hover:bg-glass">
        <LogoTile provider={host.provider} />
        <div className="min-w-0">
          <p className="truncate text-[0.8125rem] font-medium text-foreground">{host.name}</p>
          <p className="truncate font-mono text-[0.6875rem] text-faint">{accountFacts(t, { host, result, spent })}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <AccountActions test={test} overBudget={isOverBudget(host, spent)} onTest={onTest} onEdit={onEdit} onRemove={onRemove} />
        </div>
      </div>
      {result && <TestDetails result={result} />}
    </div>
  );
}

function credentialText(t: T, c: Credential): string {
  return c.kind === "cliProfile" ? t("cloud.account.cliProfile", { profile: c.profile }) : t(`cloud.account.${c.kind}`);
}

/** The account's facts line: region, sign-in, auto-stop, then the last latency and the budget. */
function accountFacts(t: T, { host, result, spent }: { host: ServerHost; result: ServerTest | undefined; spent: number | null | undefined }): string {
  const facts = [
    host.host,
    credentialText(t, credentialOf(host)),
    host.autoStopHours ? t("cloud.account.autoStop", { hours: host.autoStopHours }) : t("cloud.account.noAutoStop"),
  ].join(" · ");
  const latency = result?.latencyMs == null ? "" : ` · ${String(result.latencyMs)} ms`;
  const limit = budgetOf(host);
  if (limit === null) return facts + latency;
  const budget = ` · ${t("cloud.account.budget", { amount: formatUsd(limit, t.locale, { whole: true }) })}`;
  const spend = spent == null ? "" : `, ${t("cloud.account.spentThisMonth", { amount: formatUsd(spent, t.locale) })}`;
  return facts + latency + budget + spend;
}

/** The test's status and the row's buttons, or the remove confirmation in their place. */
function AccountActions({
  test,
  overBudget,
  onTest,
  onEdit,
  onRemove,
}: {
  test: TestState;
  overBudget: boolean;
  onTest: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  if (confirming)
    return (
      <>
        <span className="text-[0.75rem] text-muted-foreground">{t("cloud.account.removeConfirm")}</span>
        <Button variant="destructive" size="xs" onClick={onRemove}>
          {t("cloud.account.remove")}
        </Button>
        <IconButton
          label={t("cloud.account.cancel")}
          onClick={() => {
            setConfirming(false);
          }}
        >
          <X className="size-3.5" />
        </IconButton>
      </>
    );
  const testing = test === "testing";
  return (
    <>
      <TestStatus test={test} overBudget={overBudget} />
      <Button variant="outline" size="xs" onClick={onTest} disabled={testing}>
        {testing ? <Spinner className="size-3" /> : <Zap className="size-3" />} {t("cloud.account.test")}
      </Button>
      <IconButton label={t("cloud.account.edit")} onClick={onEdit}>
        <Pencil className="size-3.5" />
      </IconButton>
      <IconButton
        label={t("cloud.account.remove")}
        onClick={() => {
          setConfirming(true);
        }}
      >
        <Trash2 className="size-3.5" />
      </IconButton>
    </>
  );
}

/** Over budget first, then the test: running, never run, or its outcome. */
function TestStatus({ test, overBudget }: { test: TestState; overBudget: boolean }) {
  const t = useT();
  if (overBudget)
    return (
      <StatusPill tone="fail" pulse={test === "testing"}>
        {t("cloud.account.status.overBudget")}
      </StatusPill>
    );
  if (test === "testing")
    return (
      <StatusPill tone="muted" pulse>
        {t("cloud.account.status.testing")}
      </StatusPill>
    );
  if (!test) return <StatusPill tone="muted">{t("cloud.account.status.notTested")}</StatusPill>;
  const outcome = outcomeOf(test);
  return <StatusPill tone={outcome}>{t(OUTCOME_LABEL[outcome])}</StatusPill>;
}

/** Under the row: each check of the last test, or its message when it ran none. */
function TestDetails({ result }: { result: ServerTest }) {
  if (result.checks.length === 0)
    return (
      <p className="flex items-start gap-2 px-4 pb-3 pl-[4.125rem] text-[0.75rem] text-muted-foreground">
        <CheckGlyph state={result.ok ? "ok" : "fail"} />
        <span className="min-w-0 break-words">{result.message}</span>
      </p>
    );
  return (
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
