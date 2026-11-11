"use client";

import { useEffect, useState } from "react";
import { labRunOptions, type LabRunOptions, type Runtime } from "@/lib/tauri";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** A backend answer about a lab as read: its `value` (null until it answers, when it can't, or
 *  without the lab's source), whether it is still being read (`pending`), and why it couldn't be. */
export type LabAnswer<A> = { value: A | null; pending: boolean; error: string | null };

/** The backend's answer `ask` gives for lab `labId` (its run options, its start place), from its
 *  spec at the catalogue's commit (see [`LabAnswer`]; a failure is logged too). A lab without a
 *  source (imported from a folder) is never asked. */
export function useLabAnswer<A>(
  labId: string,
  rt: LabRuntimeInfo | null,
  ask: (id: string, runtime: Runtime, repository: string, commit: string) => Promise<A>,
): LabAnswer<A> {
  const [answer, setAnswer] = useState<{ key: string; value: A | null; error: string | null } | null>(null);
  const [runtime, repository, commit] = [rt?.runtime, rt?.repository, rt?.commit];
  const key = runtime && repository && commit ? `${labId}:${runtime}@${repository}/${commit}` : null;
  useEffect(() => {
    if (!key || !runtime || !repository || !commit) return;
    let live = true;
    ask(labId, runtime, repository, commit)
      .then((value) => {
        if (live) setAnswer({ key, value, error: null });
      })
      .catch((e: unknown) => {
        warn("reading where the lab runs from its spec")(e);
        if (live) setAnswer({ key, value: null, error: describeError(e) });
      });
    return () => {
      live = false;
    };
  }, [labId, key, runtime, repository, commit, ask]);
  const current = answer?.key === key ? answer : null;
  return { value: current?.value ?? null, pending: key !== null && current === null, error: current?.error ?? null };
}

/** Every way lab `labId` can start, as the backend reads it from its spec (`labRunOptions`). */
export const useLabRunOptions = (labId: string, rt: LabRuntimeInfo | null): LabAnswer<LabRunOptions> => useLabAnswer(labId, rt, labRunOptions);
