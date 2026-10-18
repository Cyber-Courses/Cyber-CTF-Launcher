"use client";

import { useEffect, useState } from "react";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { budgetedAccounts, cloudAccounts, isOverBudget, labsOnAccounts } from "@/features/cloud/cloud-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { serverMonthToDateCost, serverOpenSetup, serverRemove, type ServerHost } from "@/lib/tauri";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { ignore } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** Month-to-date spend, only for accounts that set a budget (Cost Explorer costs per call), read
 *  with each account's own credentials: the figure the launch budget gate checks. `hosts` is the
 *  saved list as loaded, so the spend is read again only when it reloads. */
function useMonthSpend(hosts: ServerHost[] | null) {
  const [spend, setSpend] = useState<Partial<Record<string, number | null>>>({});
  useEffect(() => {
    for (const { host } of budgetedAccounts(hosts ?? [])) {
      serverMonthToDateCost(host.id)
        .then((c) => {
          setSpend((s) => ({ ...s, [host.id]: c }));
        })
        .catch(ignore("the spend is shown only when it can be read"));
    }
  }, [hosts]);
  return spend;
}

/** The Cloud page's state and actions: the accounts, their tests and spend, the labs running on them. */
export function useCloudPage() {
  const { list, error, setError, reload } = useServerList();
  const allHosts = list?.hosts ?? null;
  const hosts = allHosts ? cloudAccounts(allHosts) : null;
  const { tests, test } = useHostTests();
  const spend = useMonthSpend(allHosts);
  const { labs, statuses, refreshStatus } = useLabs();
  // Through the shared lab actions: the lab reads busy everywhere (this list, Labs, its page)
  // until its status after the stop is read, so Stop can't be pressed twice.
  const { runs, stop } = useLabActions(refreshStatus);
  const fail = (e: unknown) => {
    setError(describeError(e));
  };

  async function stopLab(l: Lab) {
    await stop(l);
    // A failure ends the run's log with "✗ <why>": show it here too.
    const last = getDeploySnapshot().runs[l.id]?.logs.at(-1);
    if (last?.startsWith("✗")) setError(last.slice(1).trim());
  }

  return {
    hosts,
    error,
    tests,
    spend,
    statuses,
    running: labsOnAccounts(labs ?? [], statuses, hosts ?? []),
    over: (hosts ?? []).filter((h) => isOverBudget(h, spend[h.id])),
    budgeted: budgetedAccounts(hosts ?? []),
    isBusy: (l: Lab) => !!runs[l.id]?.busy,
    openSetup: (id: string | null = null) => {
      serverOpenSetup(id, "cloud").catch(fail);
    },
    remove: (id: string) => {
      serverRemove(id).then(reload, fail);
    },
    stopLab: (l: Lab) => {
      void stopLab(l);
    },
    test: (id: string) => {
      void test(id);
    },
  };
}

export type CloudPage = ReturnType<typeof useCloudPage>;
