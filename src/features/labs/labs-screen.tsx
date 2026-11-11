"use client";

import { useMemo, useState } from "react";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LIST_SCREEN } from "@/components/ui/screen-width";
import { emulatorReady } from "@/features/labs/lab-runtime";
import { preferredFirst } from "@/features/machine/hypervisors";
import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import { LabsFilterBar } from "@/features/labs/labs-filter-bar";
import { LabListSkeleton, LabsCatalogue, LabsHeader, SignInBanner, UnlistedRunning, type RowContext } from "@/features/labs/labs-list";
import { runningUnlisted } from "@/features/labs/lab-row-state";
import { ImportedLabs } from "@/features/labs/imported-labs-list";
import { OpenLab, OpenLabLoading, useDetailTitle, useOpenLab } from "@/features/labs/open-lab";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { useActiveOperations } from "@/features/labs/runs/operations";
import type { LabStatus, SystemReport } from "@/lib/tauri";
import { useSavedHosts } from "@/features/servers/use-server-list";
import { useVmProvider } from "@/lib/use-vm-provider";
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
  onLogin?: () => Promise<boolean>;
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
  const { labs, imported, completed } = catalogue;
  const statuses: Statuses = catalogue.statuses;
  const servers = useSavedHosts().hosts;
  const actions = useLabActions(catalogue.refreshStatus, servers);
  // Operations running in the backend (started from another window or before a reload), for the rows.
  const operations = useActiveOperations();
  // A lab page opens on a catalogue lab or an imported one (each read on its own).
  const openable = useMemo(() => (labs || imported ? [...(labs ?? []), ...(imported ?? [])] : null), [labs, imported]);
  const [detailSlug, setDetailSlug] = useOpenLab(openLab, openable);
  const [filters, setFilters] = useState<LabFilters>(NO_FILTERS);
  // Local hypervisors ready for a lab VM (Vagrant + hypervisor), the Settings default first, as it
  // changes in any window.
  const vmProvider = useVmProvider();
  const readyVms = useMemo(() => (report ? preferredFirst(report.readyHypervisors, vmProvider) : []), [report, vmProvider]);
  const shown = useFilteredLabs(labs, filters, statuses, completed);
  const detail = useOpenDetail(openable, detailSlug, !labs);
  useDetailTitle(detail?.title ?? null, onDetailChange);
  return { ...catalogue, statuses, actions, operations, detailSlug, setDetailSlug, filters, setFilters, servers, readyVms, shown, detail };
}

/** The open lab (`slug`) among the `openable` ones. While the catalogue is `reloading` (a sign-in
 *  or a sign-out reads it again), the lab open before stays open: its page isn't swapped for a
 *  loading one and back, which would drop what it was doing (the start a "Sign in to start"
 *  carries on once signed in). */
function useOpenDetail(openable: Lab[] | null, slug: string | null, reloading: boolean): Lab | undefined {
  const found = openable && slug ? openable.find((l) => l.slug === slug) : undefined;
  const [last, setLast] = useState<Lab | undefined>(found);
  if (found && found !== last) setLast(found);
  return found ?? (reloading && slug && last?.slug === slug ? last : undefined);
}

/** The rows' context: the screen's state and its stable handlers (the rows are memoized). */
function useRowContext(v: ReturnType<typeof useLabsScreen>, { loggedIn, onLogin, hostArch, report }: ViewProps): RowContext {
  const { actions, setDetailSlug } = v;
  const onOpen = useStableCallback((lab: Lab) => {
    setDetailSlug(lab.slug);
  });
  const onStop = useStableCallback((lab: Lab) => void actions.stop(lab));
  const onResume = useStableCallback((lab: Lab) => void actions.resume(lab));
  const login = useStableCallback(() => onLogin?.() ?? Promise.resolve(false));
  return {
    statuses: v.statuses,
    runs: actions.runs,
    completed: v.completed,
    operations: v.operations,
    report,
    servers: v.servers,
    loggedIn,
    // Signed out, the banner above the list signs in: no Sign in button on every row.
    onLogin: loggedIn && onLogin ? login : undefined,
    hostArch,
    emulates: emulatorReady(v.readyVms),
    onOpen,
    onStop,
    onResume,
  };
}

function LabsView(props: ViewProps) {
  const { loggedIn, authReady = true, onLogin, hostArch, report, onRetry } = props;
  const v = useLabsScreen(props);
  const { labs, statuses, actions, detail, setDetailSlug } = v;
  const ctx = useRowContext(v, props);

  // An imported lab's page needs no catalogue: it opens while the catalogue can't be read too.
  if (v.error && !detail?.imported)
    return <LabsUnreachable statuses={statuses} scanned={v.scanned} labs={labs} imported={v.imported} ctx={ctx} onRetry={onRetry} />;
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
        // An imported lab runs without Cyber CTF: its page never asks to sign in.
        onLogin={detail.imported ? undefined : onLogin}
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

  // The catalogue stops at LIST_SCREEN; a lab's page (above) takes the shell's full width.
  return (
    <div className={cn("space-y-5", LIST_SCREEN)}>
      <LabsHeader labs={labs} completed={v.completed} isRunning={(l) => isRunning(statuses, l)} loggedIn={loggedIn} />
      {!loggedIn && authReady && onLogin && <SignInBanner onLogin={onLogin} />}
      <ImportedLabs labs={v.imported} ctx={ctx} />
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
 *  last loaded, else by id), and the imported labs, which need no catalogue, still open, above the
 *  error and its Retry. */
function LabsUnreachable({
  statuses,
  scanned,
  labs,
  imported,
  ctx,
  onRetry,
}: {
  statuses: Statuses;
  scanned: Set<string>;
  labs: Lab[] | null;
  imported: Lab[] | null;
  ctx: RowContext;
  onRetry: () => void;
}) {
  const t = useT();
  const importedIds = new Set((imported ?? []).map((l) => l.id));
  return (
    <div className={cn("space-y-5", LIST_SCREEN)}>
      <UnlistedRunning ids={runningUnlisted(null, statuses, scanned).filter((id) => !importedIds.has(id))} labs={labs} />
      <ImportedLabs labs={imported} ctx={ctx} />
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
