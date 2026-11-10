"use client";

import { useEffect, useState } from "react";
import { labRunOptions, type LabRunOptions, type Runtime } from "@/lib/tauri";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { warn } from "@/lib/failure";

/** The backend's answer `ask` gives for lab `labId` (its run options, its start place), from its
 *  spec at the catalogue's commit; null until it answers, when it can't (the failure logged), or
 *  without the lab's source. */
export function useLabAnswer<A>(
  labId: string,
  rt: LabRuntimeInfo | null,
  ask: (id: string, runtime: Runtime, repository: string, commit: string) => Promise<A>,
): A | null {
  const [answer, setAnswer] = useState<{ key: string; value: A } | null>(null);
  const [runtime, repository, commit] = [rt?.runtime, rt?.repository, rt?.commit];
  const key = runtime && repository && commit ? `${labId}:${runtime}@${repository}/${commit}` : null;
  useEffect(() => {
    if (!key || !runtime || !repository || !commit) return;
    let live = true;
    ask(labId, runtime, repository, commit)
      .then((value) => {
        if (live) setAnswer({ key, value });
      })
      .catch(warn("reading where the lab runs from its spec"));
    return () => {
      live = false;
    };
  }, [labId, key, runtime, repository, commit, ask]);
  return answer?.key === key ? answer.value : null;
}

/** Every way lab `labId` can start, as the backend reads it from its spec (`labRunOptions`); null
 *  while it reads, or when it can't. */
export const useLabRunOptions = (labId: string, rt: LabRuntimeInfo | null): LabRunOptions | null => useLabAnswer(labId, rt, labRunOptions);
