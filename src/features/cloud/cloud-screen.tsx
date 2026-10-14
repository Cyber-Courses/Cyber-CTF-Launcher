"use client";

/** The Cloud page: connect a cloud account (AWS, Azure or GCP) and run labs as throwaway
 *  instances in it. Kept separate from the Server page so each evolves on its own. */

import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { Panel } from "@/components/ui/panel";
import { Button } from "@/components/ui/button";
import { NoticePanel } from "@/components/ui/notice-panel";
import { PageHeader } from "@/components/ui/page-header";
import { Spinner } from "@/components/ui/spinner";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { serverMonthToDateCost, serverOpenSetup, serverRemove } from "@/lib/tauri";
import { FirstRun } from "@/features/cloud/first-run";
import { AccountsPanel, BudgetPanel, RunningCloudLabs } from "@/features/cloud/cloud-panels";
import { budgetedAccounts, cloudAccounts, isOverBudget, labsOnAccounts } from "@/features/cloud/cloud-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { ignore } from "@/lib/failure";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { getDeploySnapshot } from "@/lib/deploy-store";
import { useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

export function CloudScreen() {
  const t = useT();
  const { list, error, setError, reload } = useServerList();
  const allHosts = list?.hosts ?? null;
  const hosts = allHosts ? cloudAccounts(allHosts) : null;
  const { tests, test } = useHostTests();
  const [spend, setSpend] = useState<Record<string, number | null>>({});
  const { labs, statuses, refreshStatus } = useLabs();
  // Through the shared lab actions: the lab reads busy everywhere (this list, Labs, its page)
  // until its status after the stop is read, so Stop can't be pressed twice.
  const { runs, stop } = useLabActions(refreshStatus);

  // Month-to-date spend, only for accounts that set a budget (Cost Explorer costs per call), read
  // with each account's own credentials: the figure the launch budget gate checks.
  useEffect(() => {
    (allHosts ?? [])
      .filter((h) => h.provider === "aws" && h.monthlyLimit)
      .forEach((h) => {
        serverMonthToDateCost(h.id)
          .then((c) => setSpend((s) => ({ ...s, [h.id]: c })))
          .catch(ignore("the spend is shown only when it can be read"));
      });
  }, [allHosts]);

  const openSetup = (id?: string) => serverOpenSetup(id ?? null, "cloud").catch((e) => setError(describeError(e)));

  async function remove(id: string) {
    try {
      await serverRemove(id);
      reload();
    } catch (e) {
      setError(describeError(e));
    }
  }

  async function stopLab(l: Lab) {
    await stop(l);
    // A failure ends the run's log with "✗ <why>": show it here too.
    const last = getDeploySnapshot().runs[l.id]?.logs.at(-1);
    if (last?.startsWith("✗")) setError(last.slice(1).trim());
  }

  const over = (hosts ?? []).filter((h) => isOverBudget(h, spend[h.id]));
  const running = labsOnAccounts(labs ?? [], statuses, hosts ?? []);

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("cloud.screen.title")}
        lead={t("cloud.screen.lead")}
        actions={
          hosts && hosts.length > 0 ? (
            <Button variant="outline" size="sm" onClick={() => openSetup()}>
              <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
            </Button>
          ) : undefined
        }
      />

      {error && <NoticePanel tone="fail">{error}</NoticePanel>}

      {over.length > 0 && (
        <NoticePanel tone="fail">
          {over.length === 1 ? t("cloud.screen.overOne", { name: over[0].name }) : t("cloud.screen.overMany", { count: over.length })}
        </NoticePanel>
      )}

      {running.length > 0 && <RunningCloudLabs labs={running} statuses={statuses} isBusy={(l) => !!runs[l.id]?.busy} onStop={(l) => void stopLab(l)} />}

      {hosts === null ? (
        <Panel>
          <div className="flex items-center gap-2 px-4 py-4 text-[0.8125rem] text-muted-foreground">
            <Spinner className="size-4" /> {t("cloud.screen.loading")}
          </div>
        </Panel>
      ) : hosts.length === 0 ? (
        <FirstRun onSetup={() => openSetup()} />
      ) : (
        <AccountsPanel
          accounts={hosts}
          tests={tests}
          spend={spend}
          onTest={(id) => void test(id)}
          onEdit={(id) => openSetup(id)}
          onRemove={(id) => void remove(id)}
          onSetup={() => openSetup()}
        />
      )}

      {budgetedAccounts(hosts ?? []).map((h) => (
        <BudgetPanel key={h.id} host={h} spent={spend[h.id]} />
      ))}
    </div>
  );
}
