"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { SERVER_CHANGED, serverList, serverTest, type ServerHost, type ServerList } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { describeError } from "@/lib/errors";
import { useLatest } from "@/lib/use-latest";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";
import { failedTest, type HostTest, type HostTests } from "@/features/servers/servers-model";

/** Tests a server or cloud account; never rejects (a failure to test reads as a failed test). */
export const testHost = (id: string): Promise<HostTest> => serverTest(id).catch(failedTest);

/**
 * The saved servers and cloud accounts, read on mount, again with `reload()`, and whenever the
 * setup window says one changed (`onChanged` first). `onLoaded` sees each list read. Only the
 * newest read is applied: the one on mount can answer after the one a change asked for. A read
 * that fails keeps the last list and sets `error`, which the page shows; with `tellFailure` (a
 * page that has no place to show it) the player is told in a toast instead. It is never "no
 * servers".
 */
export function useServerList({
  onLoaded,
  onChanged,
  tellFailure = false,
}: { onLoaded?: (list: ServerList) => void; onChanged?: () => void; tellFailure?: boolean } = {}) {
  const [list, setList] = useState<ServerList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(onLoaded);
  useEffect(() => {
    loaded.current = onLoaded;
  });
  const begin = useLatest();
  const reload = useCallback(() => {
    const isLatest = begin();
    serverList()
      .then((l) => {
        if (!isLatest()) return;
        setList(l);
        setError(null);
        loaded.current?.(l);
      })
      .catch((e: unknown) => {
        if (!isLatest()) return;
        setError(describeError(e));
        if (tellFailure) tell(translate("servers.screen.listFailed"))(e);
      });
  }, [begin, tellFailure]);
  useEffect(reload, [reload]);
  // The setup window saves hosts; refresh when it says so.
  useTauriEvent(SERVER_CHANGED, () => {
    onChanged?.();
    reload();
  });
  return { list, error, setError, reload };
}

/** The saved hosts and the default server's id, for a page that only reads them (a failed read
 *  is told in a toast, the last list kept): see [`useServerList`]. */
export function useSavedHosts(): { hosts: ServerHost[]; defaultId: string | null } {
  const { list } = useServerList({ tellFailure: true });
  return { hosts: list?.hosts ?? NO_HOSTS, defaultId: list?.default ?? null };
}

const NO_HOSTS: ServerHost[] = [];

/** The last test of each host ("testing" while one runs), and `test(id)` to run one. A second
 *  test of a host supersedes the first: whichever answers last, the newest one is shown. */
export function useHostTests() {
  const [tests, setTests] = useState<HostTests>({});
  const begin = useLatest();
  const test = useCallback(
    async (id: string) => {
      const isLatest = begin(id);
      setTests((t) => ({ ...t, [id]: "testing" }));
      const r = await testHost(id);
      if (isLatest()) setTests((t) => ({ ...t, [id]: r }));
    },
    [begin],
  );
  return { tests, test };
}
