"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  afterSubmit,
  challengesFor,
  emptyProgress,
  loadObjectives,
  loadProgress,
  openHint,
  openSteps,
  submitObjective,
  withHint,
  withSteps,
  type Challenge,
  type LabProgress,
  type SubmitResult,
} from "@/features/labs/objectives/objectives";

export type ObjectivesState =
  /** Still loading the lab's objectives. */
  | { kind: "loading" }
  /** The backend has no objectives API (an older deploy): the page keeps the plain question. */
  | { kind: "unavailable" }
  /** Nothing to answer: no objectives and no question. */
  | { kind: "none" }
  /** `progressRead`: the player's progress is in (or failed, or signed out: there is none to read). */
  | { kind: "ready"; challenges: Challenge[]; progress: LabProgress; signedIn: boolean; progressRead: boolean };

type Loaded = { labId: string; challenges: Challenge[] | null | "unavailable" };
type Progress = { key: string; progress: LabProgress | null };

/** The lab's challenges, kept with the lab they were read for (undefined while loading). */
function useChallenges(labId: string, question: string | null, title: string) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  useEffect(() => {
    let alive = true;
    loadObjectives(labId)
      .then((objectives) => {
        if (alive) setLoaded({ labId, challenges: challengesFor(objectives, question, title) });
      })
      .catch(() => {
        if (alive) setLoaded({ labId, challenges: "unavailable" });
      });
    return () => {
      alive = false;
    };
  }, [labId, question, title]);
  return loaded?.labId === labId ? loaded.challenges : undefined;
}

/** What a progress answer is kept with: the lab and the sign-in it was read under. */
const progressKeyOf = (labId: string, loggedIn: boolean) => `${labId}:${String(loggedIn)}`;

/** The player's progress as last read or updated, kept with the lab and sign-in (`key`) it is
 *  for; `refresh` reads it again. */
function useStoredProgress(labId: string, loggedIn: boolean) {
  const [stored, setStored] = useState<Progress | null>(null);

  const refresh = useCallback(async () => {
    if (!loggedIn) return;
    const key = progressKeyOf(labId, loggedIn);
    await loadProgress(labId)
      .then((progress) => {
        setStored({ key, progress });
      })
      .catch(() => {
        setStored((s) => (s?.key === key ? s : { key, progress: null }));
      });
  }, [labId, loggedIn]);

  useEffect(() => {
    let alive = true;
    const key = progressKeyOf(labId, loggedIn);
    if (loggedIn)
      loadProgress(labId)
        .then((progress) => {
          if (alive) setStored({ key, progress });
        })
        // Unreadable: counted as read (nothing solved, or what this page recorded since) rather than waited for.
        .catch(() => {
          if (alive) setStored((s) => (s?.key === key ? s : { key, progress: null }));
        });
    return () => {
      alive = false;
    };
  }, [labId, loggedIn]);

  return { stored, setStored, refresh };
}

/** What the page shows of the objectives, from the challenges read (undefined while loading). */
export function objectivesState(
  challenges: Challenge[] | null | "unavailable" | undefined,
  progress: LabProgress,
  signedIn: boolean,
  progressRead: boolean,
): ObjectivesState {
  if (challenges === undefined) return { kind: "loading" };
  if (challenges === "unavailable") return { kind: "unavailable" };
  if (!challenges) return { kind: "none" };
  return { kind: "ready", challenges, progress, signedIn, progressRead };
}

/**
 * Loads a lab's challenges and the player's progress, and wraps the two mutations. Progress is
 * updated in place from each answer, then read again from the backend after a correct one.
 * Each answer is kept with the lab (and sign-in) it was read for, so a page switching labs
 * never shows the previous lab's.
 */
export function useObjectives(labId: string, question: string | null, title: string, loggedIn: boolean) {
  const challenges = useChallenges(labId, question, title);
  const progressKey = progressKeyOf(labId, loggedIn);
  const { stored, setStored, refresh } = useStoredProgress(labId, loggedIn);
  const list = useMemo(() => (Array.isArray(challenges) ? challenges : []), [challenges]);
  const progress = (stored?.key === progressKey ? stored.progress : null) ?? emptyProgress(list);

  // Each update starts from the latest progress for this lab (or nothing yet).
  const update = useCallback(
    (change: (p: LabProgress) => LabProgress) => {
      setStored((s) => ({ key: progressKey, progress: change((s?.key === progressKey ? s.progress : null) ?? emptyProgress(list)) }));
    },
    [progressKey, list, setStored],
  );

  const submit = useCallback(
    async (key: string, answer: string): Promise<SubmitResult> => {
      const result = await submitObjective(labId, key, answer);
      update((p) => afterSubmit(p, list, key, result));
      if (result.correct) void refresh();
      return result;
    },
    [labId, list, update, refresh],
  );

  const showSteps = useCallback(
    async (key: string) => {
      await openSteps(labId, key);
      update((p) => withSteps(p, key));
    },
    [labId, update],
  );

  const revealHint = useCallback(
    async (key: string, index: number) => {
      const text = await openHint(labId, key, index);
      update((p) => withHint(p, { key, index, text }));
    },
    [labId, update],
  );

  const state = objectivesState(challenges, progress, loggedIn, !loggedIn || stored?.key === progressKey);
  return { state, submit, showSteps, revealHint };
}
