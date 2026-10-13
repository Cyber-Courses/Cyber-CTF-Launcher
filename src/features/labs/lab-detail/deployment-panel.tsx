"use client";

import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { DeploySteps } from "@/features/labs/deploy-steps";
import type { LabOperation } from "@/features/labs/lab-detail/lab-state";
import { useT } from "@/lib/i18n";

/** The re-run setup button's icon and word: a spinner while it runs. */
export function RerunSetupLabel({ running }: { running: boolean }) {
  const t = useT();
  return (
    <>
      {running ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />} {t("labs.detail.rerunSetup")}
    </>
  );
}

/** The deployment's steps; after a VM lab's setup broke, the way to run the setup again. */
export function DeploymentPanel({
  lines,
  times,
  busy,
  ready,
  where,
  operation,
  startedAt,
  liveStep,
  offerRerun,
  rerunning,
  onRerun,
}: {
  lines: string[];
  times: number[];
  busy: boolean;
  ready: boolean;
  where: string;
  operation: LabOperation;
  /** When the run began (its timers count from there). */
  startedAt?: number;
  /** The backend's step for a run with no log here. */
  liveStep?: string | null;
  offerRerun: boolean;
  rerunning: boolean;
  onRerun: () => void;
}) {
  const t = useT();
  return (
    <Panel>
      <DeploySteps lines={lines} times={times} busy={busy} ready={ready} where={where} operation={operation} startedAt={startedAt} liveStep={liveStep} />
      {/* A VM lab whose setup broke on one step (a Windows domain join timing out on a busy host)
          still has every machine built: running the setup again continues it in minutes, where
          a clean start would rebuild everything. */}
      {offerRerun && (
        <div className="flex flex-wrap items-center gap-3 border-t border-border px-4 py-3">
          <p className="min-w-0 flex-1 text-[0.75rem] text-muted-foreground">{t("labs.detail.setupStopped")}</p>
          <Button variant="primary" size="sm" onClick={onRerun}>
            <RerunSetupLabel running={rerunning} />
          </Button>
        </div>
      )}
    </Panel>
  );
}
