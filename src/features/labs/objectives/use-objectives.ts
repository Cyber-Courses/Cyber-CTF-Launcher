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

/**
 * Loads a lab's challenges and the player's progress, and wraps the two mutations. Progress is
 * updated in place from each answer, then read again from the backend after a correct one.
 * Each answer is kept with the lab (and sign-in) it was read for, so a page switching labs
 * never shows the previous lab's.
 */
export function useObjectives(labId: string, question: string | null, title: string, loggedIn: boolean) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const progressKey = `${labId}:${loggedIn}`;
  const [stored, setStored] = useState<Progress | null>(null);

  useEffect(() => {
    let alive = true;
    loadObjectives(labId)
      .then((objectives) => alive && setLoaded({ labId, challenges: challengesFor(objectives, question, title) }))
      .catch(() => alive && setLoaded({ labId, challenges: "unavailable" }));
    return () => {
      alive = false;
    };
  }, [labId, question, title]);

  const refresh = useCallback(async () => {
    if (!loggedIn) return;
    const key = `${labId}:${loggedIn}`;
    await loadProgress(labId)
      .then((progress) => setStored({ key, progress }))
      .catch(() => setStored((s) => (s?.key === key ? s : { key, progress: null })));
  }, [labId, loggedIn]);

  useEffect(() => {
    let alive = true;
    if (loggedIn)
      loadProgress(labId)
        .then((progress) => alive && setStored({ key: `${labId}:${loggedIn}`, progress }))
        // Unreadable: counted as read (nothing solved, or what this page recorded since) rather than waited for.
        .catch(() => alive && setStored((s) => (s?.key === `${labId}:${loggedIn}` ? s : { key: `${labId}:${loggedIn}`, progress: null })));
    return () => {
      alive = false;
    };
  }, [labId, loggedIn]);

  const challenges = loaded?.labId === labId ? loaded.challenges : undefined;
  const list = useMemo(() => (Array.isArray(challenges) ? challenges : []), [challenges]);
  const progress = (stored?.key === progressKey ? stored.progress : null) ?? emptyProgress(list);

  // Each update starts from the latest progress for this lab (or nothing yet).
  const update = useCallback(
    (change: (p: LabProgress) => LabProgress) =>
      setStored((s) => ({ key: progressKey, progress: change((s?.key === progressKey ? s.progress : null) ?? emptyProgress(list)) })),
    [progressKey, list],
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

  let state: ObjectivesState;
  if (challenges === undefined) state = { kind: "loading" };
  else if (challenges === "unavailable") state = { kind: "unavailable" };
  else if (!challenges) state = { kind: "none" };
  else state = { kind: "ready", challenges, progress, signedIn: loggedIn, progressRead: !loggedIn || stored?.key === progressKey };

  return { state, submit, showSteps, revealHint };
}
