"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { apiQuery, type LabStatus, type Provider, type Runtime } from "@/lib/tauri";
import { catalogueFor, refreshStatus, setCatalogue, useLabState } from "@/features/labs/lab-store";
import { useLabPolling } from "@/features/labs/lab-polling";
import type { T } from "@/lib/i18n";

export interface LabRuntimeInfo {
  runtime: Runtime;
  architectures: string[];
  providers: Provider[];
  /** Cyber CTF can run it for the player (hosted provider + a prepared snapshot). */
  hosted?: boolean;
  /** Its source at the pinned commit, for a cloud lab's cost estimate before it starts. */
  repository?: string;
  commit?: string;
}

/** A lab as the `labs` operation returns it (its fields are in src-tauri/src/account/api_operation.rs). */
export interface Lab {
  id: string;
  slug: string;
  title: string;
  description: string | null;
  question: string | null;
  difficulty: number;
  category: string;
  runtime: LabRuntimeInfo | null;
  /** Skills the lab exercises (from the capabilities it targets), for grouping. */
  skills: { id: string; name: string }[];
}

/** The catalogue (the one last loaded for `reloadKey` meanwhile), its load error, and the labs
 *  the player completed; each lab's status is read once the catalogue is in. `retry` loads it
 *  again (after it failed). */
function useCatalogue(reloadKey: unknown) {
  const [labs, setLabs] = useState<Lab[] | null>(() => catalogueFor(reloadKey));
  const [error, setError] = useState<string | null>(null);
  const [completed, setCompleted] = useState<Set<string>>(new Set());
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => {
    setError(null);
    setAttempt((n) => n + 1);
  }, []);
  useEffect(() => {
    // Cancelled when reloadKey changes (e.g. login toggles): a response in flight from the
    // previous key must not resolve last and overwrite the newer catalogue.
    let alive = true;
    apiQuery<{ labs: Lab[] }>("labs")
      .then((d) => {
        if (!alive) return;
        setCatalogue(reloadKey, d.labs);
        setLabs(d.labs);
        setError(null);
        d.labs.forEach((l) => void refreshStatus(l));
      })
      .catch((e) => alive && setError(String(e)));
    // Best effort: logged out (or a backend without the query) just shows no progress.
    apiQuery<{ myCompletedLabs: string[] }>("completedLabs")
      .then((d) => alive && setCompleted(new Set(d.myCompletedLabs)))
      .catch(() => alive && setCompleted(new Set()));
    return () => {
      alive = false;
    };
  }, [reloadKey, attempt]);
  return { labs, error, completed, retry };
}

/** The scan is authoritative for "running": OR it over each lab's probed status, so a lab the
 *  scan sees stays running even when its per-lab probe returns not-running or errors. A lab the
 *  scan does not see keeps its probed status (remote labs, stopped labs). */
export function mergeScan(statuses: Record<string, LabStatus>, scanRunning: Set<string>): Record<string, LabStatus> {
  if (scanRunning.size === 0) return statuses;
  const next: Record<string, LabStatus> = { ...statuses };
  for (const id of scanRunning) {
    const cur = next[id];
    next[id] = cur
      ? { ...cur, running: true }
      : {
          running: true,
          parked: null,
          machines: [],
          networks: [],
          url: null,
          host: null,
          expiresAt: null,
          place: null,
          provider: null,
          attacker: null,
          outputs: [],
          message: null,
          reapError: null,
        };
  }
  return next;
}

/**
 * Loads the published labs from CyberBackend and tracks each lab's local runtime status
 * (running / machines / url). Shared by the Labs screen and the Home dashboard so both
 * reflect the same state (the statuses live in lab-store). `reloadKey` re-fetches the
 * catalogue when it changes; the one last loaded for it shows meanwhile.
 */
export function useLabs(reloadKey: unknown = 0) {
  const { labs, error, completed, retry } = useCatalogue(reloadKey);
  const statuses = useLabState("statuses");
  // Labs the infrastructure scan (docker/vagrant) reports running. Authoritative for the running
  // flag: a lab is shown running when the scan sees it even if its per-lab status probe is failing
  // or hasn't run yet (e.g. right after a crash/restart).
  const scanRunning = useLabState("scanRunning");
  // Labs whose status has been asked at least once (answered or not): a page can wait for its
  // first answer instead of rendering a "not started" lab that flips to running a beat later.
  const probed = useLabState("probed");
  // One poll for the window, however many screens use this (see lab-polling).
  useLabPolling(labs);
  const mergedStatuses = useMemo(() => mergeScan(statuses, scanRunning), [statuses, scanRunning]);
  return { labs, error, retry, statuses: mergedStatuses, completed, refreshStatus, probed };
}

const DIFFICULTY_KEYS = ["easy", "medium", "hard"] as const;
/** A difficulty's word (1 Easy, 2 Medium, 3 Hard), empty for none. */
export const difficultyLabel = (t: T, level: number) => {
  const key = DIFFICULTY_KEYS[level - 1];
  return key ? t(`labs.difficulty.${key}`) : "";
};
