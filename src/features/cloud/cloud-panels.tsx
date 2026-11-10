"use client";

import { Square } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { Meter } from "@/components/ui/meter";
import { StatusDot, StatusPill } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import type { Lab } from "@/features/labs/use-labs";
import { AccountRow } from "@/features/cloud/account-row";
import { MAIN_PROVIDERS, isOverBudget } from "@/features/cloud/cloud-model";
import type { CloudProvider, LabStatus, ServerHost, ServerTest } from "@/lib/tauri";
import { useFormat, useT } from "@/lib/i18n";
import { formatUsd } from "@/lib/format";
import { cloudLogo } from "@/features/servers/host-setup/constants";

/** The labs running on a cloud account now (they bill while they run), each with a Stop. */
export function RunningCloudLabs({
  labs,
  statuses,
  isBusy,
  onStop,
}: {
  labs: Lab[];
  statuses: Partial<Record<string, LabStatus>>;
  isBusy: (lab: Lab) => boolean;
  onStop: (lab: Lab) => void;
}) {
  const t = useT();
  const format = useFormat();
  return (
    <Panel>
      <PanelHeader title={t("cloud.screen.runningNow")} meta={t("cloud.screen.billingWhileRunning")} />
      {labs.map((l) => {
        const s = statuses[l.id];
        return (
          <div
            key={l.id}
            className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2 transition-colors first:border-t-0 hover:bg-glass"
          >
            <StatusDot tone="ok" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[0.8125rem] font-medium">{l.title}</p>
              <p className="truncate font-mono text-[0.6875rem] text-faint">
                {s?.host}
                {s?.expiresAt ? ` · ${t("cloud.screen.autoStops", { time: format.date(s.expiresAt * 1000, { hour: "2-digit", minute: "2-digit" }) })}` : ""}
              </p>
            </div>
            <Button
              variant="destructive"
              size="xs"
              onClick={() => {
                onStop(l);
              }}
              disabled={isBusy(l)}
            >
              {isBusy(l) ? <Spinner className="size-3" /> : <Square className="size-3" />} {t("cloud.screen.stop")}
            </Button>
          </div>
        );
      })}
    </Panel>
  );
}

/** The connected accounts, and only those: a provider not connected yet is offered apart, in
 *  `ConnectProviders`, so it never reads as an account. */
export function AccountsPanel({
  accounts,
  tests,
  spend,
  onTest,
  onEdit,
  onRemove,
}: {
  accounts: ServerHost[];
  tests: Partial<Record<string, ServerTest | "testing">>;
  spend: Partial<Record<string, number | null>>;
  onTest: (id: string) => void;
  onEdit: (id: string) => void;
  onRemove: (id: string) => void;
}) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("cloud.screen.accounts")} meta={t("cloud.screen.billedOnly")} />
      {accounts.map((h) => (
        <AccountRow
          key={h.id}
          host={h}
          test={tests[h.id]}
          spent={spend[h.id]}
          onTest={() => {
            onTest(h.id);
          }}
          onEdit={() => {
            onEdit(h.id);
          }}
          onRemove={() => {
            onRemove(h.id);
          }}
        />
      ))}
    </Panel>
  );
}

/** The main providers not connected yet, each a button that opens the setup on it. */
export function ProviderButtons({ providers, onConnect }: { providers: typeof MAIN_PROVIDERS; onConnect: (provider: CloudProvider) => void }) {
  const t = useT();
  return (
    <div className="flex flex-wrap gap-2">
      {providers.map((p) => (
        <Button
          key={p.id}
          variant="outline"
          size="sm"
          aria-label={t("cloud.connect.provider", { provider: p.label })}
          onClick={() => {
            onConnect(p.id);
          }}
        >
          {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
          <img src={cloudLogo(p.id)} alt="" className="size-4 object-contain" draggable={false} />
          {p.label}
        </Button>
      ))}
    </div>
  );
}

/** Under the accounts, apart from them: the main providers not connected yet, as actions. */
export function ConnectProviders({ providers, onConnect }: { providers: typeof MAIN_PROVIDERS; onConnect: (provider: CloudProvider) => void }) {
  const t = useT();
  if (providers.length === 0) return null;
  return (
    <section aria-label={t("cloud.connect.title")} className="space-y-2.5">
      <h3 className="text-[0.8125rem] font-medium text-muted-foreground">{t("cloud.connect.title")}</h3>
      <ProviderButtons providers={providers} onConnect={onConnect} />
    </section>
  );
}

/** An account's budget guard: this month's spend against its limit, and its auto-stop. */
export function BudgetPanel({ host, limit, spent }: { host: ServerHost; limit: number; spent: number | null | undefined }) {
  const t = useT();
  const format = useFormat();
  return (
    <Panel>
      <PanelHeader title={t("cloud.screen.budgetGuard", { name: host.name })} meta={format.date(new Date(), { month: "long" })} />
      <div className="grid gap-3 px-4 pt-4 pb-5">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <span className="serif-title text-[1.75rem] leading-none text-foreground">
            {spent != null ? formatUsd(spent, t.locale) : "$…"}{" "}
            <small className="font-sans text-[0.8125rem] tracking-normal text-faint">{t("cloud.screen.ofLimit", { limit: formatUsd(limit, t.locale) })}</small>
          </span>
          <span className="flex items-center gap-2 font-mono text-[0.6875rem] text-faint">
            {isOverBudget(host, spent) && <StatusPill tone="fail">{t("cloud.screen.overBudget")}</StatusPill>}
            {host.autoStopHours ? t("cloud.screen.autoStopAfter", { hours: host.autoStopHours }) : t("cloud.screen.noAutoStop")}
          </span>
        </div>
        <Meter value={spent == null ? 0 : (spent / limit) * 100} />
      </div>
    </Panel>
  );
}
