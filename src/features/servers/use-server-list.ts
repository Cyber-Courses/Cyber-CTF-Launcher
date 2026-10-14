"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { SERVER_CHANGED, serverList, serverTest, type ServerList } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { describeError } from "@/lib/errors";
import { failedTest, type HostTest } from "@/features/servers/servers-model";

/** Tests a server or cloud account; never rejects (a failure to test reads as a failed test). */
export const testHost = (id: string): Promise<HostTest> => serverTest(id).catch(failedTest);

/**
 * The saved servers and cloud accounts, read on mount, again with `reload()`, and whenever the
 * setup window says one changed (`onChanged` first). `onLoaded` sees each list read.
 */
export function useServerList({ onLoaded, onChanged }: { onLoaded?: (list: ServerList) => void; onChanged?: () => void } = {}) {
  const [list, setList] = useState<ServerList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(onLoaded);
  useEffect(() => {
    loaded.current = onLoaded;
  });
  const reload = useCallback(() => {
    serverList()
      .then((l) => {
        setList(l);
        setError(null);
        loaded.current?.(l);
      })
      .catch((e) => setError(describeError(e)));
  }, []);
  useEffect(reload, [reload]);
  // The setup window saves hosts; refresh when it says so.
  useTauriEvent(SERVER_CHANGED, () => {
    onChanged?.();
    reload();
  });
  return { list, error, setError, reload };
}

/** The last test of each host ("testing" while one runs), and `test(id)` to run one. */
export function useHostTests() {
  const [tests, setTests] = useState<Record<string, HostTest | "testing">>({});
  const test = useCallback(async (id: string) => {
    setTests((t) => ({ ...t, [id]: "testing" }));
    const r = await testHost(id);
    setTests((t) => ({ ...t, [id]: r }));
  }, []);
  return { tests, test };
}
