"use client";

import { useEffect, useMemo, useState } from "react";
import { apiQuery, runningLabs, type LabStatus, type Provider, type Runtime } from "@/lib/tauri";
import { catalogueFor, refreshStatus, setCatalogue, setScanRunning, useLabState } from "@/features/labs/lab-store";
import { ignore } from "@/lib/failure";
import type { T } from "@/lib/i18n";

export interface LabRuntimeInfo {
  runtime: Runtime;
  architectures: string[];
  providers: Provider[];
  /** Cyber CTF can run it for the player (hosted provider + a prepared snapshot). */
  hosted?: boolean;
}

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

const LABS_QUERY = `{ labs(sort: [{ title: ASC }]) {
  id slug title description question difficulty category
  runtime { runtime architectures providers hosted }
  skills { id name }
} }`;

/** The labs this player has solved (correct evidence submitted on the website). */
const COMPLETED_QUERY = `{ myCompletedLabs }`;

/**
 * Loads the published labs from CyberBackend and tracks each lab's local runtime status
 * (running / machines / url). Shared by the Labs screen and the Home dashboard so both
 * reflect the same state (the statuses live in lab-store). `reloadKey` re-fetches the
 * catalogue when it changes; the one last loaded for it shows meanwhile.
 */
export function useLabs(reloadKey: unknown = 0) {
  const [labs, setLabs] = useState<Lab[] | null>(() => catalogueFor(reloadKey));
  const [error, setError] = useState<string | null>(null);
  const statuses = useLabState("statuses");
  const [completed, setCompleted] = useState<Set<string>>(new Set());
  // Labs the infrastructure scan (docker/vagrant) reports running. Authoritative for the running
  // flag: a lab is shown running when the scan sees it even if its per-lab status probe is failing
  // or hasn't run yet (e.g. right after a crash/restart).
  const scanRunning = useLabState("scanRunning");
  // Labs whose status has been asked at least once (answered or not): a page can wait for its
  // first answer instead of rendering a "not started" lab that flips to running a beat later.
  const probed = useLabState("probed");

  useEffect(() => {
    // Cancelled when reloadKey changes (e.g. login toggles): a response in flight from the
    // previous key must not resolve last and overwrite the newer catalogue.
    let alive = true;
    apiQuery<{ labs: Lab[] }>(LABS_QUERY)
      .then((d) => {
        if (!alive) return;
        setCatalogue(reloadKey, d.labs);
        setLabs(d.labs);
        setError(null);
        d.labs.forEach((l) => void refreshStatus(l));
      })
      .catch((e) => alive && setError(String(e)));
    // Best effort: logged out (or a backend without the query) just shows no progress.
    apiQuery<{ myCompletedLabs: string[] }>(COMPLETED_QUERY)
      .then((d) => alive && setCompleted(new Set(d.myCompletedLabs)))
      .catch(() => alive && setCompleted(new Set()));
    return () => {
      alive = false;
    };
  }, [reloadKey]);

  // Poll local status so labs launched from the website (claimed + run by the agent)
  // surface here within a few seconds without a manual refresh.
  useEffect(() => {
    if (!labs) return;
    const t = setInterval(() => labs.forEach((l) => void refreshStatus(l)), 6000);
    return () => clearInterval(t);
  }, [labs]);

  // Reconcile what is actually running from the infrastructure itself (docker + vagrant), so a
  // running lab is detected after a crash/restart, and a successful launch never falls back to a
  // "Start lab" page just because its per-lab status probe briefly fails. The scan is ground
  // truth; refreshStatus then fills in each running lab's machines and URL.
  useEffect(() => {
    let alive = true;
    // One scan at a time: a slow one (a wedged hypervisor) must not have the interval stack more.
    let scanning = false;
    const scan = () => {
      if (scanning) return;
      scanning = true;
      const began = Date.now();
      runningLabs()
        .then((all) => {
          if (!alive) return;
          // A lab read fresh after this scan began (an operation just ended on it) is left out.
          const ids = setScanRunning(all, began);
          (labs ?? []).filter((l) => ids.includes(l.id)).forEach((l) => void refreshStatus(l));
        })
        .catch(ignore("scanned again in a moment"))
        .finally(() => {
          scanning = false;
        });
    };
    scan();
    const t = setInterval(scan, 6000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [labs]);

  // The scan is authoritative for "running": OR it over each lab's probed status, so a lab the
  // scan sees stays running even when its per-lab probe returns not-running or errors. A lab the
  // scan does not see keeps its probed status (remote labs, stopped labs).
  const mergedStatuses = useMemo(() => {
    if (scanRunning.size === 0) return statuses;
    const next: Record<string, LabStatus> = { ...statuses };
    for (const id of scanRunning) {
      const cur = next[id];
      next[id] = cur
        ? { ...cur, running: true }
        : { running: true, parked: null, machines: [], networks: [], url: null, host: null, expiresAt: null, place: null, provider: null, attacker: null };
    }
    return next;
  }, [statuses, scanRunning]);

  return { labs, error, statuses: mergedStatuses, completed, refreshStatus, probed };
}

const DIFFICULTY_KEYS = ["easy", "medium", "hard"] as const;
/** A difficulty's word (1 Easy, 2 Medium, 3 Hard), empty for none. */
export const difficultyLabel = (t: T, level: number) => {
  const key = DIFFICULTY_KEYS[level - 1];
  return key ? t(`labs.difficulty.${key}`) : "";
};
