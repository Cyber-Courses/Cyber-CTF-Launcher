"use client";

import { useEffect, useMemo, useState } from "react";
import { useLabs, type Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { budgetedAccounts, cloudAccounts, labsOnAccounts } from "@/features/cloud/cloud-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { serverMonthToDateCost, serverOpenSetup, serverRemove, type CloudProvider, type MonthSpend, type ServerHost } from "@/lib/tauri";
import { ignore } from "@/lib/failure";
import { describeError } from "@/lib/errors";
import { confirmDestructive } from "@/lib/confirm";
import { useT } from "@/lib/i18n";

/** Month-to-date spend and the budget's verdict, only for accounts that set a budget (Cost
 *  Explorer costs per call), read with each account's own credentials: the figure the launch
 *  budget gate checks. `budgeted` comes from the saved list as loaded, so the spend is read again
 *  only when it reloads. */
function useMonthSpend(budgeted: { host: ServerHost }[]) {
  const [spend, setSpend] = useState<Partial<Record<string, MonthSpend>>>({});
  useEffect(() => {
    for (const { host } of budgeted) {
      serverMonthToDateCost(host.id)
        .then((c) => {
          setSpend((s) => ({ ...s, [host.id]: c }));
        })
        .catch(ignore("the spend is shown only when it can be read"));
    }
  }, [budgeted]);
  return spend;
}

const NO_HOSTS: ServerHost[] = [];

/** The Cloud page's state and actions: the accounts, their tests and spend, the labs running on them. */
export function useCloudPage() {
  const t = useT();
  const { list, error, setError, reload } = useServerList();
  const allHosts = list?.hosts ?? null;
  const hosts = allHosts ? cloudAccounts(allHosts) : null;
  const { tests, test } = useHostTests();
  const budgeted = useMemo(() => (list ? budgetedAccounts(list.hosts, list.budgets) : []), [list]);
  const spend = useMonthSpend(budgeted);
  const { labs, statuses, refreshStatus } = useLabs();
  // Through the shared lab actions: the lab reads busy everywhere (this list, Labs, its page)
  // until its status after the stop is read, so Stop can't be pressed twice.
  const { runs, stop } = useLabActions(refreshStatus, allHosts ?? NO_HOSTS);
  const fail = (e: unknown) => {
    setError(describeError(e));
  };

  /** Removes account `id`, and only it, once the player confirmed it in the system's dialog. */
  async function removeAccount(id: string) {
    const name = hosts?.find((h) => h.id === id)?.name ?? id;
    const yes = await confirmDestructive({
      title: t("cloud.account.removeTitle", { name }),
      message: t("cloud.account.removeBody"),
      confirm: t("cloud.account.removeAccount"),
    });
    if (yes) await serverRemove(id).then(reload, fail);
  }

  async function stopLab(l: Lab) {
    // Why the stop failed or was refused (another operation on the lab), shown here too.
    const why = await stop(l);
    if (why) setError(why);
  }

  return {
    hosts,
    error,
    tests,
    spend,
    statuses,
    running: labsOnAccounts(labs ?? [], statuses, hosts ?? []),
    over: (hosts ?? []).filter((h) => spend[h.id]?.over),
    budgets: list?.budgets ?? {},
    budgeted,
    isBusy: (l: Lab) => !!runs[l.id]?.busy,
    /** The cloud setup: editing account `id`, or a new account, on `provider` when one is named. */
    openSetup: (id: string | null = null, provider: CloudProvider | null = null) => {
      serverOpenSetup(id, "cloud", provider).catch(fail);
    },
    remove: (id: string) => {
      void removeAccount(id);
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
