"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { serverCapacity, serverOpenSetup, serverRunningLabs, systemCheck, type HostCapacity, type SystemReport } from "@/lib/tauri";
import { onPremHosts } from "@/features/servers/servers-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { ignore, warn } from "@/lib/failure";
import { usePolled } from "@/lib/use-poll";

/**
 * What the Servers page shows of its hosts: the on-prem hosts and the default, each one's last
 * test, capacity and running labs, and this machine's system report. Every host is tested and
 * its capacity probed as it first loads. Also the page's actions: open the setup window, test
 * them all, and run a change then reload (`act`), its failure shown as the page's error.
 */
export function useServerHosts() {
  const { tests, test } = useHostTests();
  const [caps, setCaps] = useState<Record<string, HostCapacity | null>>({});
  const [report, setReport] = useState<SystemReport | null>(null);

  const loadCapacity = useCallback((id: string) => {
    serverCapacity(id)
      .then((c) => setCaps((m) => ({ ...m, [id]: c })))
      .catch(ignore("capacity is shown only when it can be read"));
  }, []);

  // Running-labs-per-host, refreshed on a slow poll (labs start and stop from other screens).
  const [running, loadRunning] = usePolled(() => serverRunningLabs().then((r) => r ?? {}), 8000, {} as Record<string, number>, {
    onError: ignore("read again on the next server change"),
  });

  // Auto-test and probe capacity for every host as it loads, so the row is live without a
  // click. The `seen` guard keeps a host from re-running these on every load; a change saved in
  // the setup window re-probes them all.
  const seen = useRef<Set<string>>(new Set());
  const { list, error, setError, reload } = useServerList({
    onLoaded: (l) => {
      // Only the hosts this page lists: the cloud accounts are tested on the Cloud page.
      for (const h of onPremHosts(l.hosts)) {
        if (!seen.current.has(h.id)) {
          seen.current.add(h.id);
          void test(h.id);
          loadCapacity(h.id);
        }
      }
    },
    onChanged: () => {
      seen.current.clear();
      loadRunning();
    },
  });
  // Cloud accounts (AWS/Azure/GCP) live on the Cloud page; the Servers page is on-prem hosts only.
  const hosts = list ? onPremHosts(list.hosts) : null;
  const defaultId = list?.default ?? null;

  useEffect(() => {
    systemCheck().then(setReport).catch(warn("system check"));
  }, []);

  const open = (id: string | null) => serverOpenSetup(id, "server").catch((e) => setError(String(e)));
  const testAll = () => hosts?.forEach((h) => test(h.id));
  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      reload();
    } catch (e) {
      setError(String(e));
    }
  }

  return { hosts, defaultId, tests, test, caps, running, loadRunning, report, error, open, testAll, act };
}
