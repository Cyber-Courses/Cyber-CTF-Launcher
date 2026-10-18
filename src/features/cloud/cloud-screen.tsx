"use client";

/** The Cloud page: connect a cloud account (AWS, Azure or GCP) and run labs as throwaway
 *  instances in it. Kept separate from the Server page so each evolves on its own. */

import { Plus } from "lucide-react";
import { Panel } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { NoticePanel } from "@/components/ui/notice-panel";
import { PageHeader } from "@/components/ui/page-header";
import { Spinner } from "@/components/ui/spinner";
import { FirstRun } from "@/features/cloud/first-run";
import { AccountsPanel, BudgetPanel, RunningCloudLabs } from "@/features/cloud/cloud-panels";
import { useCloudPage, type CloudPage } from "@/features/cloud/use-cloud-page";
import type { ServerHost } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

export function CloudScreen() {
  const t = useT();
  const page = useCloudPage();
  const { hosts, error, running } = page;
  const setUp = () => {
    page.openSetup();
  };
  return (
    <div className="space-y-5">
      <PageHeader
        title={t("cloud.screen.title")}
        lead={t("cloud.screen.lead")}
        actions={
          hosts && hosts.length > 0 ? (
            <Button variant="outline" size="sm" onClick={setUp}>
              <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
            </Button>
          ) : undefined
        }
      />
      {error && <NoticePanel tone="fail">{error}</NoticePanel>}
      <OverBudgetNotice over={page.over} />
      {running.length > 0 && <RunningCloudLabs labs={running} statuses={page.statuses} isBusy={page.isBusy} onStop={page.stopLab} />}
      <Accounts page={page} onSetup={setUp} />
      {page.budgeted.map(({ host, limit }) => (
        <BudgetPanel key={host.id} host={host} limit={limit} spent={page.spend[host.id]} />
      ))}
    </div>
  );
}

function OverBudgetNotice({ over }: { over: ServerHost[] }) {
  const t = useT();
  if (over.length === 0) return null;
  return (
    <NoticePanel tone="fail">
      {over.length === 1 ? t("cloud.screen.overOne", { name: over[0].name }) : t("cloud.screen.overMany", { count: over.length })}
    </NoticePanel>
  );
}

/** The accounts: loading, the first-run tiles before any, else the list. */
function Accounts({ page, onSetup }: { page: CloudPage; onSetup: () => void }) {
  const t = useT();
  if (page.hosts === null)
    return (
      <Panel>
        <div className="flex items-center gap-2 px-4 py-4 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("cloud.screen.loading")}
        </div>
      </Panel>
    );
  if (page.hosts.length === 0) return <FirstRun onSetup={onSetup} />;
  return (
    <AccountsPanel
      accounts={page.hosts}
      tests={page.tests}
      spend={page.spend}
      onTest={page.test}
      onEdit={page.openSetup}
      onRemove={page.remove}
      onSetup={onSetup}
    />
  );
}
