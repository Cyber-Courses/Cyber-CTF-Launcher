"use client";

import type { ComponentProps } from "react";
import { LogIn } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { PageHeader } from "@/components/ui/page-header";
import { StatusDot } from "@/components/ui/status-pill";
import { Skeleton } from "@/components/ui/skeleton";
import { LabRow } from "@/features/labs/lab-row";
import { needsCloudAccount, setupNeeded } from "@/features/labs/lab-readiness";
import type { Lab } from "@/features/labs/use-labs";
import { startFailed, type DeployRun } from "@/lib/deploy-store";
import type { LabStatus, ServerHost, SystemReport } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { prettyLabId } from "@/lib/format";
import { useT } from "@/lib/i18n";

// The Labs list: its header, the placeholder while the catalogue loads, and the rows (running
// labs pinned on top).

/** The lab list's placeholder rows while the catalogue loads. */
export function LabListSkeleton() {
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

/** The title, with how many labs there are, run and are solved once the catalogue is in (the
 *  solved count only when signed in: signed out, nothing is tracked). */
export function LabsHeader({
  labs,
  completed,
  isRunning,
  loggedIn = true,
}: {
  labs: Lab[] | null;
  completed: Set<string>;
  isRunning: (lab: Lab) => boolean;
  loggedIn?: boolean;
}) {
  const t = useT();
  const solvedCount = labs ? labs.filter((l) => completed.has(l.id)).length : 0;
  const runningCount = labs ? labs.filter(isRunning).length : 0;
  return (
    <PageHeader
      title={t("labs.list.title")}
      lead={
        labs ? (
          <span className="font-mono text-[0.75rem] tabular-nums">
            {loggedIn
              ? t("labs.list.summary", { total: labs.length, running: runningCount, solved: solvedCount })
              : t("labs.list.summarySignedOut", { total: labs.length, running: runningCount })}
          </span>
        ) : (
          t("labs.list.intro")
        )
      }
    />
  );
}

/** Signed out: one sign-in prompt above the list, instead of a Sign in button on every row. */
export function SignInBanner({ onLogin }: { onLogin: () => Promise<void> }) {
  const t = useT();
  return (
    <Panel>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2.5 px-4 py-3.5">
        <div className="min-w-0 flex-1 basis-64">
          <p className="text-[0.8125rem] font-medium text-foreground">{t("labs.list.signInTitle")}</p>
          <p className="mt-0.5 text-[0.75rem] text-muted-foreground">{t("labs.list.signInBody")}</p>
        </div>
        <Button size="sm" onClick={() => void onLogin().catch(tell(t("labs.row.signInFailed")))}>
          <LogIn className="size-3.5" /> {t("labs.row.signIn")}
        </Button>
      </div>
    </Panel>
  );
}

/** What a row of the list reads besides its lab. The handlers keep their identity across
 *  renders (the rows are memoized). */
export type RowContext = {
  statuses: Record<string, LabStatus>;
  runs: Record<string, DeployRun>;
  completed: Set<string>;
  deploying: Set<string>;
  report: SystemReport | null | undefined;
  servers: ServerHost[];
  loggedIn: boolean;
  onLogin?: () => Promise<void>;
  hostArch: string;
  emulates: boolean;
  onOpen: (lab: Lab) => void;
  onStop: (lab: Lab) => void;
  onResume: (lab: Lab) => void;
};

/** A lab row's props in a list (this one, the Overview's: they read the same), with what this
 *  machine is missing to run it. Values and the context's stable handlers only, so the memoized
 *  row skips a render that changes nothing for its lab. */
export function catalogueRowProps(lab: Lab, c: RowContext): ComponentProps<typeof LabRow> {
  const running = !!c.statuses[lab.id]?.running;
  return {
    lab,
    status: c.statuses[lab.id],
    busy: !!c.runs[lab.id]?.busy,
    operation: c.runs[lab.id]?.op,
    loggedIn: c.loggedIn,
    onLogin: c.onLogin,
    hostArch: c.hostArch,
    emulates: c.emulates,
    solved: c.completed.has(lab.id),
    deploying: c.deploying.has(lab.id),
    failed: startFailed(c.runs[lab.id]),
    setup: running ? null : setupNeeded(lab, c.report ?? null, c.servers),
    connectCloud: needsCloudAccount(lab, c.servers),
    onOpen: c.onOpen,
    onStop: c.onStop,
    onResume: c.onResume,
  };
}

/** One lab of the list. */
function CatalogueRow({ lab, ctx }: { lab: Lab; ctx: RowContext }) {
  return <LabRow {...catalogueRowProps(lab, ctx)} />;
}

/** The list: a placeholder while it loads, a note when empty or nothing matches, else the running
 *  labs on top and the rest below. */
export function LabsCatalogue({ labs, filtered, running, rest, ctx }: { labs: Lab[] | null; filtered: Lab[]; running: Lab[]; rest: Lab[]; ctx: RowContext }) {
  const t = useT();
  if (!labs) return <LabListSkeleton />;
  if (labs.length === 0) return <EmptyState icon="labs" title={t("labs.list.emptyTitle")} description={t("labs.list.emptyDescription")} />;
  if (filtered.length === 0) return <EmptyState icon="labs" title={t("labs.list.noMatchTitle")} description={t("labs.list.noMatchDescription")} />;
  const row = (lab: Lab) => <CatalogueRow key={lab.id} lab={lab} ctx={ctx} />;
  return (
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
  );
}

/** A lab running here that the catalogue doesn't list (it can't be reached): its id made
 *  readable, so a running lab never vanishes from the Overview with the catalogue. */
export function UnlistedRunningRow({ id, title }: { id: string; title?: string }) {
  const t = useT();
  return (
    <div className="grid h-13 grid-cols-[auto_minmax(0,1fr)] items-center gap-3.5 border-t border-border px-4 text-[0.8125rem] first:border-t-0">
      <span className="flex w-2 justify-center">
        <StatusDot tone="ok" />
      </span>
      <span className="min-w-0">
        <span className="block truncate font-medium text-foreground">{title ?? prettyLabId(id)}</span>
        <span className="block truncate font-mono text-[0.6875rem] text-faint">
          {id} · {t("home.running.noDetails")}
        </span>
      </span>
    </div>
  );
}

/** The labs running here, by id, as their own "Running now" panel: while the catalogue can't be
 *  read (titled from the one last loaded, when there is one). */
export function UnlistedRunning({ ids, labs = null }: { ids: string[]; labs?: Lab[] | null }) {
  const t = useT();
  if (ids.length === 0) return null;
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone="ok" /> {t("labs.list.runningNow")}
          </>
        }
        meta={t("labs.list.labCount", { count: ids.length })}
      />
      {ids.map((id) => (
        <UnlistedRunningRow key={id} id={id} title={labs?.find((l) => l.id === id)?.title} />
      ))}
    </Panel>
  );
}
