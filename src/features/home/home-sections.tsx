"use client";

import type { ReactNode } from "react";
import { ArrowRight, Cloud, ExternalLink, Play, RotateCw, Server, TriangleAlert } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { PageHeader } from "@/components/ui/page-header";
import type { Lab } from "@/features/labs/use-labs";
import { CalloutRow, StatCard } from "@/features/machine/machine-parts";
import { usage, type UsageHistory } from "@/features/machine/machine-status";
import { greetingKey, heroStatus } from "@/features/home/home-model";
import type { Capacity } from "@/features/home/capacity";
import { machineOpenSetup, type AuthStatus, type MachineMetrics } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useFormat, useT } from "@/lib/i18n";

// The Overview's sections, top to bottom: the greeting, live usage, the low RAM callout, the
// running labs and the labs preview. The screen itself (home-screen.tsx) wires them up.

/** The greeting, the status line under it and the main call to action (set up or browse). */
export function HomeHeader({
  auth,
  now,
  dockerReady,
  running,
  checkFailed = false,
  onBrowse,
}: {
  auth: AuthStatus | null;
  now: number;
  dockerReady: boolean | null;
  running: number;
  /** The machine check failed (no report): say so rather than "Checking this machine…". */
  checkFailed?: boolean;
  onBrowse: () => void;
}) {
  const t = useT();
  const name = auth?.name?.split(" ")[0];
  const hello = t(greetingKey(new Date(now).getHours()));
  return (
    <PageHeader
      title={
        auth?.loggedIn ? (
          name ? (
            <>{t.rich("home.title.named", { em: (s) => <em>{s}</em> }, { greeting: hello, name })}</>
          ) : (
            t("home.title.plain", { greeting: hello })
          )
        ) : (
          <>{t.rich("home.title.welcome", { em: (s) => <em>{s}</em> })}</>
        )
      }
      lead={t(`home.status.${heroStatus(dockerReady, running, checkFailed)}`, { count: running })}
      actions={
        dockerReady === false ? (
          <Button size="sm" onClick={() => machineOpenSetup().catch(tell(t("home.actions.setUpFailed")))}>
            <Play className="size-3.5" /> {t("home.actions.setUp")}
          </Button>
        ) : (
          <Button size="sm" onClick={onBrowse}>
            {t("home.actions.browse")} <ArrowRight className="size-3.5" />
          </Button>
        )
      }
    />
  );
}

/** CPU, memory and disk now, each with its sparkline; "unavailable" once reading them failed
 *  (none read yet), not placeholders forever. */
export function UsageStats({ metrics, history, failed = false }: { metrics: MachineMetrics | null; history: UsageHistory; failed?: boolean }) {
  const t = useT();
  const format = useFormat();
  /** Gigabytes with one decimal, without a trailing ".0" (19.5, 32). */
  const gb = (bytes: number) => format.number(bytes / 1e9, { maximumFractionDigits: 1, useGrouping: false });
  const use = metrics ? usage(metrics) : null;
  const gone = failed && !metrics;
  const unavailable = gone ? t("home.stats.unavailable") : "";
  return (
    <div className="grid gap-4 sm:grid-cols-3">
      <StatCard
        label={t("home.stats.cpu")}
        detail={metrics ? t("home.stats.cores", { cores: metrics.cores }) : unavailable}
        value={metrics ? metrics.cpu : null}
        history={history.cpu}
        unavailable={gone}
      />
      <StatCard
        label={t("home.stats.memory")}
        detail={metrics ? t("home.stats.usage", { used: gb(metrics.memUsed), total: gb(metrics.memTotal) }) : unavailable}
        value={use && metrics?.memTotal ? use.memPct : null}
        history={history.mem}
        unavailable={gone}
      />
      <StatCard
        label={t("home.stats.disk")}
        detail={metrics ? t("home.stats.usage", { used: gb(metrics.diskUsed), total: gb(metrics.diskTotal) }) : unavailable}
        value={use && metrics?.diskTotal ? use.diskPct : null}
        history={history.disk}
        unavailable={gone}
      />
    </div>
  );
}

/** Too little RAM for labs: says so and points to a cloud or server instead. */
export function LowRamCallout({ capacity, onNavigate }: { capacity: Capacity; onNavigate: (tab: "cloud" | "server") => void }) {
  const t = useT();
  const format = useFormat();
  return (
    <CalloutRow
      tone="warn"
      icon={<TriangleAlert className="size-4" />}
      title={capacity.title}
      meta={t("home.capacity.total", { total: format.number(capacity.totalGB, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}
      actions={
        <>
          <Button variant="outline" size="xs" onClick={() => onNavigate("cloud")}>
            <Cloud /> {t("home.capacity.cloud")}
          </Button>
          <Button variant="outline" size="xs" onClick={() => onNavigate("server")}>
            <Server /> {t("home.capacity.server")}
          </Button>
        </>
      }
    >
      {capacity.detail}
    </CalloutRow>
  );
}

/** The labs running now, with how many lab containers they hold. */
export function RunningPanel({ running, containers, children }: { running: number; containers: number; children: ReactNode }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader
        title={t("home.running.title")}
        meta={
          <span className="tabular-nums">
            {t("home.running.labs", { count: running })}
            {containers > 0 ? ` · ${t("home.running.containers", { count: containers })}` : ""}
          </span>
        }
      />
      {running > 0 ? children : <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("home.running.empty")}</p>}
    </Panel>
  );
}

/** The first labs of the catalog, with a link to all of them; why it couldn't load, with a retry. */
export function LabsPreviewPanel({
  labs,
  error = null,
  onRetry,
  onAll,
  children,
}: {
  labs: Lab[] | null;
  /** Loading the catalogue failed. */
  error?: string | null;
  onRetry?: () => void;
  onAll: () => void;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            {t("home.labs.title")} {labs && <span className="font-mono text-[0.6875rem] font-normal text-faint">{labs.length}</span>}
          </>
        }
        action={
          <Button variant="ghost" size="xs" onClick={onAll}>
            {t("home.labs.all")} <ArrowRight />
          </Button>
        }
      />
      {!labs && error ? (
        <div role="alert" className="flex flex-wrap items-center gap-x-4 gap-y-2.5 px-4 py-3.5">
          <StatusDot tone="fail" />
          <div className="min-w-0 flex-1 basis-64">
            <p className="text-[0.8125rem] font-medium text-foreground">{t("labs.list.unreachableTitle")}</p>
            <p className="mt-0.5 text-[0.75rem] text-muted-foreground">{t("labs.list.unreachableDescription")}</p>
          </div>
          {onRetry && (
            <Button variant="outline" size="sm" onClick={onRetry}>
              <RotateCw className="size-3.5" /> {t("labs.list.retry")}
            </Button>
          )}
        </div>
      ) : !labs ? (
        <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("home.labs.loading")}</p>
      ) : labs.length === 0 ? (
        <p className="px-4 py-3.5 text-[0.8125rem] text-muted-foreground">{t("home.labs.empty")}</p>
      ) : (
        children
      )}
    </Panel>
  );
}

/** Signed out: signing in here lets labs started on the website run on this machine. */
export function SignInHint() {
  const t = useT();
  return (
    <p className="flex items-center gap-1.5 text-[0.75rem] text-muted-foreground">
      <ExternalLink className="size-3.5" /> {t("home.signInHint")}
    </p>
  );
}
