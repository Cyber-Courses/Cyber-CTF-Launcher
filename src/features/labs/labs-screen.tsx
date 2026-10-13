"use client";

import { useMemo, useState } from "react";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LIST_SCREEN } from "@/components/ui/screen-width";
import { EMULATORS, readyHypervisors } from "@/features/labs/lab-runtime";
import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import { LabsFilterBar } from "@/features/labs/labs-filter-bar";
import { LabListSkeleton, LabsCatalogue, LabsHeader, SignInBanner, UnlistedRunning } from "@/features/labs/labs-list";
import { runningUnlisted } from "@/features/labs/lab-row-state";
import { OpenLab, OpenLabLoading, useDetailTitle, useOpenLab } from "@/features/labs/open-lab";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useDeployingLabs } from "@/lib/deploy-store";
import type { LabStatus, SystemReport } from "@/lib/tauri";
import { useSavedServers } from "@/features/labs/use-saved-servers";
import { getVmProvider } from "@/lib/settings";
import { useStableCallback } from "@/lib/use-stable-callback";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** The labs the filters keep: running ones pinned on top, the rest following in one list, by title. */
function useFilteredLabs(labs: Lab[] | null, filters: LabFilters, statuses: Record<string, LabStatus>, completed: Set<string>) {
  const filtered = useMemo(() => (labs ? filterLabs(labs, filters, (l) => !!statuses[l.id]?.running, completed) : []), [labs, filters, statuses, completed]);
  const running = filtered.filter((l) => !!statuses[l.id]?.running);
  const rest = useMemo(() => filtered.filter((l) => !statuses[l.id]?.running).sort((a, b) => a.title.localeCompare(b.title)), [filtered, statuses]);
  return { filtered, running, rest };
}

type LabsProps = {
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
};

/** The Labs screen: the catalogue as a filterable list (running labs pinned on top), or one
 *  lab's page when one is open. Retry (after the catalogue failed to load) mounts it afresh,
 *  which loads the catalogue again. */
export function Labs(props: LabsProps) {
  const [attempt, setAttempt] = useState(0);
  return <LabsView key={attempt} {...props} onRetry={() => setAttempt((n) => n + 1)} />;
}

/** The labs screen while the machine check (which it needs) hasn't answered: its header and
 *  placeholder rows, the shape of what is coming. */
export function LabsLoading() {
  return (
    <div className={cn("space-y-5", LIST_SCREEN)} aria-busy="true">
      <LabsHeader labs={null} completed={new Set()} isRunning={() => false} />
      <LabListSkeleton />
    </div>
  );
}

function LabsView({ loggedIn, authReady = true, onLogin, hostArch, report, openLab, onDetailChange, onRetry }: LabsProps & { onRetry: () => void }) {
  const t = useT();
  const { labs, error, statuses, completed, refreshStatus, probed } = useLabs(loggedIn);
  const actions = useLabActions(refreshStatus);
  // Deploys running in the backend (started from another window or before a reload), for the rows' dots.
  const deploying = useDeployingLabs();
  const [detailSlug, setDetailSlug] = useOpenLab(openLab, labs);
  const [filters, setFilters] = useState<LabFilters>(NO_FILTERS);
  const servers = useSavedServers();
  // Local hypervisors ready for a lab VM (Vagrant + hypervisor), the Settings default first.
  const readyVms = useMemo(() => (report ? readyHypervisors(report, getVmProvider()) : []), [report]);
  const { filtered, running, rest } = useFilteredLabs(labs, filters, statuses, completed);
  const detail = labs && detailSlug ? labs.find((l) => l.slug === detailSlug) : undefined;
  useDetailTitle(detail?.title ?? null, onDetailChange);
  // Stable, for the memoized rows (see RowContext).
  const onOpen = useStableCallback((lab: Lab) => setDetailSlug(lab.slug));
  const onStop = useStableCallback((lab: Lab) => void actions.stop(lab));
  const onResume = useStableCallback((lab: Lab) => void actions.resume(lab));
  const login = useStableCallback(() => onLogin?.() ?? Promise.resolve());

  if (error)
    return (
      <div className={cn("space-y-5", LIST_SCREEN)}>
        {/* The labs running here still run: listed while the catalogue can't be read (by the
            catalogue last loaded, else by id). */}
        <UnlistedRunning ids={runningUnlisted(null, statuses)} labs={labs} />
        <EmptyState
          icon="alert"
          title={t("labs.list.unreachableTitle")}
          description={t("labs.list.unreachableDescription")}
          action={
            <Button variant="outline" size="sm" onClick={onRetry}>
              <RotateCw className="size-3.5" /> {t("labs.list.retry")}
            </Button>
          }
        />
      </div>
    );
  if (detailSlug && !detail && !labs) return <OpenLabLoading />;
  if (detail) {
    return (
      <OpenLab
        // Its own state per lab (a pending action, an open remove dialog, the attack box's
        // auto-start): opening another lab from the sidebar or palette must not inherit it.
        key={detail.id}
        lab={detail}
        status={statuses[detail.id]}
        actions={actions}
        loggedIn={loggedIn}
        onLogin={onLogin}
        hostArch={hostArch}
        readyVms={readyVms}
        report={report}
        ready={authReady && (!detail.runtime || probed.has(detail.id))}
        onBack={() => setDetailSlug(null)}
      />
    );
  }

  const ctx = {
    statuses,
    runs: actions.runs,
    completed,
    deploying,
    report,
    servers,
    loggedIn,
    // Signed out, the banner above the list signs in: no Sign in button on every row.
    onLogin: loggedIn && onLogin ? login : undefined,
    hostArch,
    emulates: readyVms.some((p) => EMULATORS.includes(p)),
    onOpen,
    onStop,
    onResume,
  };
  // The catalogue stops at LIST_SCREEN; a lab's page (above) takes the shell's full width.
  return (
    <div className={cn("space-y-5", LIST_SCREEN)}>
      <LabsHeader labs={labs} completed={completed} isRunning={(l) => !!statuses[l.id]?.running} loggedIn={loggedIn} />
      {!loggedIn && authReady && onLogin && <SignInBanner onLogin={onLogin} />}
      <LabsFilterBar filters={filters} onChange={(patch) => setFilters((f) => ({ ...f, ...patch }))} />
      <LabsCatalogue labs={labs} filtered={filtered} running={running} rest={rest} ctx={ctx} />
    </div>
  );
}
