"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { EmptyState } from "@/components/ui/empty-state";
import { Spinner } from "@/components/ui/spinner";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { PageHeader } from "@/components/ui/page-header";
import { StatusDot } from "@/components/ui/status-pill";
import { Skeleton } from "@/components/ui/skeleton";
import { LabRow } from "@/features/labs/lab-row";
import { EMULATORS, readyHypervisors } from "@/features/labs/lab-runtime";
import { LabDetail } from "@/features/labs/lab-detail";
import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import { LabsFilterBar } from "@/features/labs/labs-filter-bar";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { needsCloudAccount, setupNeeded } from "@/features/labs/lab-readiness";
import { useDeployingLabs } from "@/lib/deploy-store";
import { serverList, type ServerHost, type SystemReport } from "@/lib/tauri";
import { getVmProvider } from "@/lib/settings";
import { useT } from "@/lib/i18n";

/** The lab list's placeholder rows while the catalogue loads. */
function LabListSkeleton() {
  return (
    <Panel>
      {[0, 1, 2, 3, 4].map((i) => (
        <div key={i} className="flex h-13 items-center gap-3.5 border-t border-border px-4 first:border-t-0">
          <Skeleton className="size-2 rounded-full" />
          <div className="flex-1">
            <Skeleton className="h-3 w-44" />
            <Skeleton className="mt-1.5 h-2.5 w-64 max-w-full" />
          </div>
          <Skeleton className="hidden h-2.5 w-20 md:block" />
          <Skeleton className="hidden h-4 w-14 rounded-full md:block" />
          <Skeleton className="h-7 w-16 rounded-full" />
        </div>
      ))}
    </Panel>
  );
}

/** The Labs screen: the catalogue as a filterable list (running labs pinned on top), or one
 *  lab's page when one is open. */
export function Labs({
  loggedIn,
  authReady = true,
  onLogin,
  hostArch,
  report,
  openLab,
  onDetailChange,
}: {
  loggedIn: boolean;
  /** False until the sign-in state is known (the lab page waits for it). */
  authReady?: boolean;
  /** Logs in from a lab (a logged-out Start). */
  onLogin?: () => Promise<void>;
  hostArch: string;
  report?: SystemReport | null;
  openLab: { slug: string | null; tick: number };
  /** Told the open lab's title (null on the list), for the shell's breadcrumb. */
  onDetailChange?: (title: string | null) => void;
}) {
  const t = useT();
  const { labs, error, statuses, completed, refreshStatus, probed } = useLabs(loggedIn);
  const { runs, launch, stop, park, resume, provision } = useLabActions(refreshStatus);
  // Deploys running in the backend (started from another window or before a reload), for the rows' dots.
  const deploying = useDeployingLabs();
  // Opened straight from the slug the navigation carried, so a lab opened from Overview, the
  // command palette or a deep link shows its page on the first render instead of flashing the
  // list first.
  const [detailSlug, setDetailSlug] = useState<string | null>(() => openLab.slug);
  const [filters, setFilters] = useState<LabFilters>(NO_FILTERS);
  const [servers, setServers] = useState<ServerHost[]>([]);
  // Local hypervisors ready for a lab VM (Vagrant + hypervisor), the Settings default first.
  const readyVms = useMemo(() => (report ? readyHypervisors(report, getVmProvider()) : []), [report]);
  const emulates = readyVms.some((p) => EMULATORS.includes(p));

  // Saved servers count as somewhere a VM lab can run (for the "needs setup" hint).
  useEffect(() => {
    serverList()
      .then((l) => setServers(l.hosts))
      .catch(() => setServers([]));
  }, []);

  // External navigation (sidebar, command palette, Overview): act only when the nav tick
  // changes, so a labs refresh never ejects the user from a detail they opened from the list.
  // A slug opens it (once labs load); no slug returns to the list.
  const lastTick = useRef(openLab.tick);
  useEffect(() => {
    if (openLab.tick === lastTick.current) return;
    // A slug that isn't in the catalogue yet: wait (don't consume the tick) until labs load.
    if (openLab.slug && !labs?.some((l) => l.slug === openLab.slug)) return;
    lastTick.current = openLab.tick;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setDetailSlug(openLab.slug);
  }, [openLab, labs]);

  const isRunning = (l: Lab) => !!statuses[l.id]?.running;

  const filtered = useMemo(() => (labs ? filterLabs(labs, filters, (l) => !!statuses[l.id]?.running, completed) : []), [labs, filters, statuses, completed]);

  // Running labs are pinned on top; the rest follow in one list, by title.
  const running = filtered.filter(isRunning);
  const rest = useMemo(() => filtered.filter((l) => !statuses[l.id]?.running).sort((a, b) => a.title.localeCompare(b.title)), [filtered, statuses]);

  const detail = labs && detailSlug ? labs.find((l) => l.slug === detailSlug) : undefined;
  const detailTitle = detail?.title ?? null;
  useEffect(() => onDetailChange?.(detailTitle), [detailTitle, onDetailChange]);
  useEffect(() => () => onDetailChange?.(null), [onDetailChange]);

  if (error) return <EmptyState icon="alert" title={t("labs.list.unreachableTitle")} description={t("labs.list.unreachableDescription")} />;

  // Opening a lab while the catalogue is still loading: show a placeholder, never the list, so
  // there is no flash of the list before the lab page.
  if (detailSlug && !detail && !labs) {
    return (
      <div className="flex items-center justify-center py-24 text-muted-foreground">
        <Spinner className="size-5" />
      </div>
    );
  }
  if (detail) {
    return (
      <LabDetail
        // Its own state per lab (a pending action, an open remove dialog, the attack box's
        // auto-start): opening another lab from the sidebar or palette must not inherit it.
        key={detail.id}
        lab={detail}
        status={statuses[detail.id]}
        busy={!!runs[detail.id]?.busy}
        logs={runs[detail.id]?.logs ?? []}
        times={runs[detail.id]?.times ?? []}
        loggedIn={loggedIn}
        onLogin={onLogin}
        hostArch={hostArch}
        onBack={() => setDetailSlug(null)}
        readyVms={readyVms}
        dockerRunning={report ? report.dockerRunning : null}
        onStart={(target) =>
          launch(
            detail,
            target.kind === "host" ? target.id : null,
            target.kind === "local-vm" ? target.provider : undefined,
            report,
            target.kind === "local" ? target.ports : undefined,
          )
        }
        onStop={() => stop(detail)}
        onPark={(mode) => park(detail, mode)}
        onResume={() => resume(detail)}
        onProvision={(machine) => provision(detail, machine)}
        ready={authReady && (!detail.runtime || probed.has(detail.id))}
      />
    );
  }

  const row = (lab: Lab) => (
    <LabRow
      key={lab.id}
      lab={lab}
      status={statuses[lab.id]}
      busy={!!runs[lab.id]?.busy}
      operation={runs[lab.id]?.op}
      loggedIn={loggedIn}
      onLogin={onLogin}
      hostArch={hostArch}
      emulates={emulates}
      solved={completed.has(lab.id)}
      deploying={deploying.has(lab.id)}
      setup={isRunning(lab) ? null : setupNeeded(lab, report ?? null, servers)}
      connectCloud={needsCloudAccount(lab, servers)}
      onOpen={() => setDetailSlug(lab.slug)}
      onStop={() => stop(lab)}
      onResume={() => resume(lab)}
    />
  );

  const solvedCount = labs ? labs.filter((l) => completed.has(l.id)).length : 0;
  const runningCount = labs ? labs.filter(isRunning).length : 0;

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("labs.list.title")}
        lead={
          labs ? (
            <span className="font-mono text-[0.75rem] tabular-nums">
              {t("labs.list.summary", { total: labs.length, running: runningCount, solved: solvedCount })}
            </span>
          ) : (
            t("labs.list.intro")
          )
        }
      />

      <LabsFilterBar filters={filters} onChange={(patch) => setFilters((f) => ({ ...f, ...patch }))} />

      {!labs ? (
        <LabListSkeleton />
      ) : labs.length === 0 ? (
        <EmptyState icon="labs" title={t("labs.list.emptyTitle")} description={t("labs.list.emptyDescription")} />
      ) : filtered.length === 0 ? (
        <EmptyState icon="labs" title={t("labs.list.noMatchTitle")} description={t("labs.list.noMatchDescription")} />
      ) : (
        <>
          {running.length > 0 && (
            <Panel>
              <PanelHeader
                title={
                  <>
                    <StatusDot tone="ok" /> {t("labs.list.runningNow")}
                  </>
                }
                meta={t("labs.list.labCount", { count: running.length })}
              />
              <div>{running.map(row)}</div>
            </Panel>
          )}
          {rest.length > 0 && (
            <Panel>
              {running.length > 0 && <PanelHeader title={t("labs.list.allLabs")} meta={`${rest.length}`} />}
              <div>{rest.map(row)}</div>
            </Panel>
          )}
        </>
      )}
    </div>
  );
}
