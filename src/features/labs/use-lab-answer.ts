"use client";

import { labRunOptions, type LabRunOptions, type Runtime } from "@/lib/tauri";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { useRead, valueOf } from "@/lib/use-read";

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
  const [runtime, repository, commit] = [rt?.runtime, rt?.repository, rt?.commit];
  const key = runtime && repository && commit ? `${labId}:${runtime}@${repository}/${commit}` : null;
  const read = useRead(key, () => ask(labId, runtime ?? "DOCKER", repository ?? "", commit ?? ""), "reading where the lab runs from its spec");
  return { value: valueOf(read), pending: read.state === "reading", error: read.state === "failed" ? read.error : null };
}

/** Every way lab `labId` can start, as the backend reads it from its spec (`labRunOptions`). */
export const useLabRunOptions = (labId: string, rt: LabRuntimeInfo | null): LabAnswer<LabRunOptions> => useLabAnswer(labId, rt, labRunOptions);
