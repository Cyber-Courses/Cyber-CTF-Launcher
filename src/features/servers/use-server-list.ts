"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { SERVER_CHANGED, serverList, serverTest, type ServerList } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { describeError } from "@/lib/errors";
import { useLatest } from "@/lib/use-latest";
import { failedTest, type HostTest, type HostTests } from "@/features/servers/servers-model";

/** Tests a server or cloud account; never rejects (a failure to test reads as a failed test). */
export const testHost = (id: string): Promise<HostTest> => serverTest(id).catch(failedTest);

/**
 * The saved servers and cloud accounts, read on mount, again with `reload()`, and whenever the
 * setup window says one changed (`onChanged` first). `onLoaded` sees each list read. Only the
 * newest read is applied: the one on mount can answer after the one a change asked for.
 */
export function useServerList({ onLoaded, onChanged }: { onLoaded?: (list: ServerList) => void; onChanged?: () => void } = {}) {
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
        if (isLatest()) setError(describeError(e));
      });
  }, [begin]);
  useEffect(reload, [reload]);
  // The setup window saves hosts; refresh when it says so.
  useTauriEvent(SERVER_CHANGED, () => {
    onChanged?.();
    reload();
  });
  return { list, error, setError, reload };
}

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
