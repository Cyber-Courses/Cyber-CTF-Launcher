"use client";

import { useMemo, useState } from "react";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LIST_SCREEN } from "@/components/ui/screen-width";
import { EMULATORS, readyHypervisors } from "@/features/labs/lab-runtime";
import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import { LabsFilterBar } from "@/features/labs/labs-filter-bar";
import { LabListSkeleton, LabsCatalogue, LabsHeader, SignInBanner, UnlistedRunning, type RowContext } from "@/features/labs/labs-list";
import { runningUnlisted } from "@/features/labs/lab-row-state";
import { OpenLab, OpenLabLoading, useDetailTitle, useOpenLab } from "@/features/labs/open-lab";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useDeployingLabs } from "@/features/labs/runs/operations";
import type { LabStatus, SystemReport } from "@/lib/tauri";
import { useSavedServers } from "@/features/labs/use-saved-servers";
import { getVmProvider } from "@/lib/settings";
import { useStableCallback } from "@/lib/use-stable-callback";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Each lab's status, by id: none for a lab not asked yet. */
type Statuses = Partial<Record<string, LabStatus>>;

/** Whether `lab`'s status says it runs (not known yet: no). */
const isRunning = (statuses: Statuses, lab: Lab) => statuses[lab.id]?.running ?? false;

/** The labs the filters keep: running ones pinned on top, the rest following in one list, by title. */
function useFilteredLabs(labs: Lab[] | null, filters: LabFilters, statuses: Statuses, completed: Set<string>) {
  const filtered = useMemo(() => (labs ? filterLabs(labs, filters, (l) => isRunning(statuses, l), completed) : []), [labs, filters, statuses, completed]);
  const running = filtered.filter((l) => isRunning(statuses, l));
  const rest = useMemo(() => filtered.filter((l) => !isRunning(statuses, l)).sort((a, b) => a.title.localeCompare(b.title)), [filtered, statuses]);
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
  return (
    <LabsView
      key={attempt}
      {...props}
      onRetry={() => {
        setAttempt((n) => n + 1);
      }}
    />
  );
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

type ViewProps = LabsProps & { onRetry: () => void };

/** Everything the Labs screen reads: the catalogue and statuses, the actions, the open lab, the
 *  filters, the saved servers and the hypervisors ready here. */
function useLabsScreen({ loggedIn, report, openLab, onDetailChange }: ViewProps) {
  const catalogue = useLabs(loggedIn);
  const { labs, completed } = catalogue;
  const statuses: Statuses = catalogue.statuses;
  const actions = useLabActions(catalogue.refreshStatus);
  // Deploys running in the backend (started from another window or before a reload), for the rows' dots.
  const deploying = useDeployingLabs();
  const [detailSlug, setDetailSlug] = useOpenLab(openLab, labs);
  const [filters, setFilters] = useState<LabFilters>(NO_FILTERS);
  const servers = useSavedServers();
  // Local hypervisors ready for a lab VM (Vagrant + hypervisor), the Settings default first.
  const readyVms = useMemo(() => (report ? readyHypervisors(report, getVmProvider()) : []), [report]);
  const shown = useFilteredLabs(labs, filters, statuses, completed);
  const detail = labs && detailSlug ? labs.find((l) => l.slug === detailSlug) : undefined;
  useDetailTitle(detail?.title ?? null, onDetailChange);
  return { ...catalogue, statuses, actions, deploying, detailSlug, setDetailSlug, filters, setFilters, servers, readyVms, shown, detail };
}

function LabsView(props: ViewProps) {
  const { loggedIn, authReady = true, onLogin, hostArch, report, onRetry } = props;
  const v = useLabsScreen(props);
  const { labs, statuses, actions, detail, setDetailSlug } = v;
  // Stable, for the memoized rows (see RowContext).
  const onOpen = useStableCallback((lab: Lab) => {
    setDetailSlug(lab.slug);
  });
  const onStop = useStableCallback((lab: Lab) => void actions.stop(lab));
  const onResume = useStableCallback((lab: Lab) => void actions.resume(lab));
  const login = useStableCallback(() => onLogin?.() ?? Promise.resolve());

  if (v.error) return <LabsUnreachable statuses={statuses} labs={labs} onRetry={onRetry} />;
  if (v.detailSlug && !detail && !labs) return <OpenLabLoading />;
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
        readyVms={v.readyVms}
        report={report}
        ready={authReady && (!detail.runtime || v.probed.has(detail.id))}
        onBack={() => {
          setDetailSlug(null);
        }}
      />
    );
  }

  const ctx: RowContext = {
    statuses,
    runs: actions.runs,
    completed: v.completed,
    deploying: v.deploying,
    report,
    servers: v.servers,
    loggedIn,
    // Signed out, the banner above the list signs in: no Sign in button on every row.
    onLogin: loggedIn && onLogin ? login : undefined,
    hostArch,
    emulates: v.readyVms.some((p) => EMULATORS.includes(p)),
    onOpen,
    onStop,
    onResume,
  };
  // The catalogue stops at LIST_SCREEN; a lab's page (above) takes the shell's full width.
  return (
    <div className={cn("space-y-5", LIST_SCREEN)}>
      <LabsHeader labs={labs} completed={v.completed} isRunning={(l) => isRunning(statuses, l)} loggedIn={loggedIn} />
      {!loggedIn && authReady && onLogin && <SignInBanner onLogin={onLogin} />}
      <LabsFilterBar
        filters={v.filters}
        onChange={(patch) => {
          v.setFilters((f) => ({ ...f, ...patch }));
        }}
      />
      <LabsCatalogue labs={labs} {...v.shown} ctx={ctx} />
    </div>
  );
}

/** The catalogue couldn't be loaded: the labs running here still run, listed (by the catalogue
 *  last loaded, else by id), above the error and its Retry. */
function LabsUnreachable({ statuses, labs, onRetry }: { statuses: Statuses; labs: Lab[] | null; onRetry: () => void }) {
  const t = useT();
  return (
    <div className={cn("space-y-5", LIST_SCREEN)}>
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
}
