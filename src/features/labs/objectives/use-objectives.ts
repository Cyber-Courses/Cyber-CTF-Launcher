"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { describeError } from "@/lib/errors";
import { warn } from "@/lib/failure";
import { useLatest } from "@/lib/use-latest";
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
  /** The objectives couldn't be read (offline, the backend failing), `why` in the player's
   *  words: the page keeps the plain question. */
  | { kind: "unavailable"; why: string }
  /** Nothing to answer: no objectives and no question. */
  | { kind: "none" }
  /** `progressRead`: the player's progress is in (or failed, or signed out: there is none to read). */
  | { kind: "ready"; challenges: Challenge[]; progress: LabProgress; signedIn: boolean; progressRead: boolean };

/** The challenges read (null: none), or why they couldn't be. */
type Challenges = Challenge[] | null | { unread: string };
type Loaded = { labId: string; challenges: Challenges };
type Progress = { key: string; progress: LabProgress | null };

/** The lab's challenges, kept with the lab they were read for (undefined while loading). */
function useChallenges(labId: string, question: string | null, title: string, enabled: boolean) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    loadObjectives(labId)
      .then((objectives) => {
        if (alive) setLoaded({ labId, challenges: challengesFor(objectives, question, title) });
      })
      .catch((e: unknown) => {
        warn("reading the lab's objectives")(e);
        if (alive) setLoaded({ labId, challenges: { unread: describeError(e) } });
      });
    return () => {
      alive = false;
    };
  }, [labId, question, title, enabled]);
  return loaded?.labId === labId ? loaded.challenges : undefined;
}

/** What a progress answer is kept with: the lab and the sign-in it was read under. */
const progressKeyOf = (labId: string, loggedIn: boolean) => `${labId}:${String(loggedIn)}`;

/** The player's progress as last read or updated, kept with the lab and sign-in (`key`) it is
 *  for; `refresh` reads it again. One read serves the first load and every refresh, and only the
 *  newest one applies: a refresh still out when the page moves to another lab never lands there. */
function useStoredProgress(labId: string, loggedIn: boolean) {
  const [stored, setStored] = useState<Progress | null>(null);
  const beginRead = useLatest();

  const refresh = useCallback(async () => {
    const latest = beginRead();
    if (!loggedIn) return;
    const key = progressKeyOf(labId, loggedIn);
    await loadProgress(labId).then(
      (progress) => {
        if (latest()) setStored({ key, progress });
      },
      // Unreadable: counted as read (nothing solved, or what this page recorded since) rather than waited for.
      warn("reading the lab's progress", () => {
        if (latest()) setStored((s) => (s?.key === key ? s : { key, progress: null }));
      }),
    );
  }, [labId, loggedIn, beginRead]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { stored, setStored, refresh };
}

/** What the page shows of the objectives, from the challenges read (undefined while loading). */
export function objectivesState(challenges: Challenges | undefined, progress: LabProgress, signedIn: boolean, progressRead: boolean): ObjectivesState {
  if (challenges === undefined) return { kind: "loading" };
  if (challenges && "unread" in challenges) return { kind: "unavailable", why: challenges.unread };
  if (!challenges) return { kind: "none" };
  return { kind: "ready", challenges, progress, signedIn, progressRead };
}

/**
 * Loads a lab's challenges and the player's progress, and wraps the two mutations. Progress is
 * updated in place from each answer, then read again from the backend after a correct one.
 * Each answer is kept with the lab (and sign-in) it was read for, so a page switching labs
 * never shows the previous lab's. Not `enabled` (a lab the backend doesn't know: one imported from
 * a folder), nothing is asked of the backend and the objectives stay loading.
 */
export function useObjectives(labId: string, question: string | null, title: string, loggedIn: boolean, enabled = true) {
  const challenges = useChallenges(labId, question, title, enabled);
  const progressKey = progressKeyOf(labId, loggedIn);
  const { stored, setStored, refresh } = useStoredProgress(labId, loggedIn && enabled);
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
