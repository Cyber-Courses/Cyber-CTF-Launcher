"use client";

import type { ReactNode } from "react";
import { ArrowRight, RotateCw } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import type { Lab } from "@/features/labs/use-labs";
import { useT } from "@/lib/i18n";

// The Overview's lab panels: the labs running now, and the first labs of the catalogue.

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
