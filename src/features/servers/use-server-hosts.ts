"use client";

import { useCallback, useRef, useState } from "react";
import { serverCapacity, serverOpenSetup, serverRunningLabs, type HostCapacity } from "@/lib/tauri";
import { onPremHosts } from "@/features/servers/servers-model";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { ignore, tell } from "@/lib/failure";
import { usePolled } from "@/lib/use-poll";
import { describeError, errorCode } from "@/lib/errors";
import { translate } from "@/lib/i18n";
import { confirmNodeKeys } from "@/features/servers/node-keys-prompt";

/** A node offering other SSH host keys than the confirmed ones is told, in the setup's words,
 *  and is true; keys that couldn't be read are left to what needs them, which says so. */
function keysRefused(e: unknown): boolean {
  const refused = errorCode(e) === "sshHostKeyRefused";
  if (refused) tell(translate("servers.setup.trust.titleChanged"))(e);
  return refused;
}

/** The labs running per host, by its id (a host with none isn't listed). */
const readRunning = (): Promise<Partial<Record<string, number>>> => serverRunningLabs();

/**
 * What the Servers page shows of its hosts: the on-prem hosts and the default, each one's last
 * test, capacity and running labs. Every host is tested and
 * its capacity probed as it first loads. Also the page's actions: open the setup window, test
 * them all, and run a change then reload (`act`), its failure shown as the page's error.
 */
export function useServerHosts() {
  const { tests, test } = useHostTests();
  const [caps, setCaps] = useState<Partial<Record<string, HostCapacity | null>>>({});

  const loadCapacity = useCallback((id: string) => {
    serverCapacity(id)
      .then((c) => {
        setCaps((m) => ({ ...m, [id]: c }));
      })
      .catch(ignore("capacity is shown only when it can be read"));
  }, []);

  // Running-labs-per-host, refreshed on a slow poll (labs start and stop from other screens).
  const [running, loadRunning] = usePolled(readRunning, 8000, {}, { onError: ignore("read again on the next server change") });

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
      void loadRunning();
    },
  });
  // Cloud accounts (AWS/Azure/GCP) live on the Cloud page; the Servers page is on-prem hosts only.
  const hosts = list ? onPremHosts(list.hosts) : null;
  const defaultId = list?.default ?? null;

  const fail = (e: unknown) => {
    setError(describeError(e));
  };
  const open = (id: string | null) => {
    serverOpenSetup(id, "server").catch(fail);
  };
  /** Test connection the player asked for (a row's Test, Test all): a server that passes and whose
   *  labs run through its node then has the node's SSH host keys confirmed, when they never were. */
  const testAsked = async (id: string) => {
    if ((await test(id)).ok) await confirmNodeKeys(id).catch(keysRefused);
  };
  /** Whether the VM test the player asked for on `id` may run: its VM is reached through the node
   *  as a lab's are, so the node's SSH host keys, never confirmed, are confirmed first, as before
   *  a start. False when the player declines, or the node offers other keys than the confirmed
   *  ones (told as at Test). */
  const vmTestAllowed = (id: string): Promise<boolean> => confirmNodeKeys(id).catch((e: unknown) => !keysRefused(e));
  const testAll = () => {
    for (const h of hosts ?? []) void testAsked(h.id);
  };
  /** Runs a change, then reloads the list; its failure is the page's error. */
  const act = (change: () => Promise<unknown>) => {
    change().then(reload, fail);
  };

  return { hosts, defaultId, tests, test, testAsked, vmTestAllowed, caps, running, loadRunning, error, open, testAll, act };
}

export type ServerHosts = ReturnType<typeof useServerHosts>;
