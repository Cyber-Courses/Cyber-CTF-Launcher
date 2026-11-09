"use client";

import { useCallback, useEffect, useState } from "react";
import { apiQuery, type Provider, type Runtime } from "@/lib/tauri";
import { catalogueFor, refreshStatus, setCatalogue, useLabState } from "@/features/labs/lab-store";
import { useLabPolling } from "@/features/labs/lab-polling";
import type { T } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

export interface LabRuntimeInfo {
  runtime: Runtime;
  architectures: string[];
  providers: Provider[];
  /** The ways the lab can run, from the backend's capability flags. Absent on an older backend,
   *  where the places are derived from `runtime` instead. */
  docker?: boolean;
  vm?: boolean;
  cloud?: boolean;
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
      .catch((e: unknown) => {
        if (alive) setError(describeError(e));
      });
    // Best effort: logged out (or a backend without the query) just shows no progress.
    apiQuery<{ myCompletedLabs: string[] }>("completedLabs")
      .then((d) => {
        if (alive) setCompleted(new Set(d.myCompletedLabs));
      })
      .catch(() => {
        if (alive) setCompleted(new Set());
      });
    return () => {
      alive = false;
    };
  }, [reloadKey, attempt]);
  return { labs, error, completed, retry };
}

/**
 * Loads the published labs from CyberBackend and tracks each lab's local runtime status
 * (running / machines / url). Shared by the Labs screen and the Home dashboard so both
 * reflect the same state (the statuses live in lab-store). `reloadKey` re-fetches the
 * catalogue when it changes; the one last loaded for it shows meanwhile.
 */
export function useLabs(reloadKey: unknown = 0) {
  const { labs, error, completed, retry } = useCatalogue(reloadKey);
  // Each lab's own status alone says whether it runs: the infrastructure scan only picks which
  // labs to read again (`hotLabs`). A lab whose launch failed has its machines up, which the scan
  // sees, but is not running.
  const statuses = useLabState("statuses");
  // What the scan sees up: only for the labs the catalogue doesn't list (`runningUnlisted`).
  const scanned = useLabState("scanRunning");
  // Labs whose status has been asked at least once (answered or not): a page can wait for its
  // first answer instead of rendering a "not started" lab that flips to running a beat later.
  const probed = useLabState("probed");
  // One poll for the window, however many screens use this (see lab-polling).
  useLabPolling(labs);
  return { labs, error, retry, statuses, scanned, completed, refreshStatus, probed };
}

const DIFFICULTY_KEYS: Partial<Record<number, "easy" | "medium" | "hard">> = { 1: "easy", 2: "medium", 3: "hard" };
/** A difficulty's word (1 Easy, 2 Medium, 3 Hard), empty for none. */
export const difficultyLabel = (t: T, level: number) => {
  const key = DIFFICULTY_KEYS[level];
  return key ? t(`labs.difficulty.${key}`) : "";
};
