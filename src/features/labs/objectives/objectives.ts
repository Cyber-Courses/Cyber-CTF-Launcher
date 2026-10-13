import { apiQuery } from "@/lib/tauri";

/**
 * A lab's objectives (CyberBackend docs/lab-objectives.md): ordered challenges, each with its
 * guided steps. A lab registered without objectives has one implicit `main` challenge, the
 * lab's question. Secrets never reach the client; hint texts come one at a time from
 * openObjectiveHelp, and opening steps or a hint is recorded (a solve without them is stronger
 * evidence for the skills graph). The GraphQL documents live on the Rust side
 * (`account/api_operation.rs`, labObjectives, labProgress, submitObjective, openObjectiveHelp).
 */
export interface ObjectiveStep {
  key: string;
  title: string;
  prompt: string | null;
  hintCount: number;
}

export interface Challenge extends ObjectiveStep {
  optional: boolean;
  points: number | null;
  steps: ObjectiveStep[];
}

export interface OpenedHint {
  key: string;
  index: number;
  text: string;
}

export interface LabProgress {
  solved: string[];
  stepsOpened: string[];
  hints: OpenedHint[];
  requiredSolved: number;
  requiredTotal: number;
  completed: boolean;
}

export interface SubmitResult {
  correct: boolean;
  alreadySolved: boolean;
  labCompleted: boolean;
}

/** The key of a lab's implicit challenge (a lab without objectives). */
export const MAIN = "main";
/** CyberBackend refuses longer submissions. */
export const MAX_ANSWER = 2000;

type RawStep = Partial<ObjectiveStep> & { key: string };
type RawChallenge = RawStep & { optional?: boolean | null; points?: number | null; steps?: RawStep[] | null };

const step = (s: RawStep): ObjectiveStep => ({ key: s.key, title: s.title ?? s.key, prompt: s.prompt ?? null, hintCount: s.hintCount ?? 0 });

/** The lab's challenges as the backend lists them (empty for a lab without objectives). */
export async function loadObjectives(labId: string): Promise<Challenge[]> {
  const d = await apiQuery<{ labs?: { id?: string; objectives?: RawChallenge[] | null }[] }>("labObjectives", { id: labId });
  const lab = d?.labs?.find((l) => l.id === labId);
  return (lab?.objectives ?? []).map((c) => ({
    ...step(c),
    optional: !!c.optional,
    points: c.points ?? null,
    steps: (c.steps ?? []).map(step),
  }));
}

/** The player's progress on the lab; null when signed out. */
export async function loadProgress(labId: string): Promise<LabProgress | null> {
  const d = await apiQuery<{ labs?: { id?: string; myProgress?: Partial<LabProgress> | null }[] }>("labProgress", { id: labId });
  const lab = d?.labs?.find((l) => l.id === labId);
  const p = lab?.myProgress;
  if (!p) return null;
  return {
    solved: p.solved ?? [],
    stepsOpened: p.stepsOpened ?? [],
    hints: p.hints ?? [],
    requiredSolved: p.requiredSolved ?? 0,
    requiredTotal: p.requiredTotal ?? 0,
    completed: !!p.completed,
  };
}

export async function submitObjective(labId: string, key: string, answer: string): Promise<SubmitResult> {
  const d = await apiQuery<{ submitObjective: SubmitResult }>("submitObjective", { labId, key, answer });
  return d.submitObjective;
}

/** Records that a challenge's steps were opened. */
export async function openSteps(labId: string, key: string): Promise<void> {
  await apiQuery("openObjectiveHelp", { labId, key, what: "STEPS", index: null });
}

/** Opens hint `index` (0-based, in order) of an objective and returns its text. */
export async function openHint(labId: string, key: string, index: number): Promise<string> {
  const d = await apiQuery<{ openObjectiveHelp: { ok: boolean; hint: string | null } }>("openObjectiveHelp", { labId, key, what: "HINT", index });
  return d.openObjectiveHelp.hint ?? "";
}

/**
 * The challenges to show: the lab's own, or its implicit `main` (the lab's question) when it
 * declares none. Null when there is nothing to answer (no objectives and no question).
 */
export function challengesFor(objectives: Challenge[], question: string | null, title: string): Challenge[] | null {
  if (objectives.length) return objectives;
  if (!question) return null;
  return [{ key: MAIN, title, prompt: question, hintCount: 0, optional: false, points: null, steps: [] }];
}

/** Progress before the player's is in (or signed out): nothing solved or opened. */
export function emptyProgress(challenges: Challenge[]): LabProgress {
  return {
    solved: [],
    stepsOpened: [],
    hints: [],
    requiredSolved: 0,
    requiredTotal: challenges.filter((c) => !c.optional).length,
    completed: false,
  };
}

/** The progress after a submit's answer, before the backend's own is read again. */
export function afterSubmit(progress: LabProgress, challenges: Challenge[], key: string, result: SubmitResult): LabProgress {
  if (!result.correct) return progress;
  const first = !progress.solved.includes(key);
  const required = challenges.some((c) => c.key === key && !c.optional);
  return {
    ...progress,
    solved: first ? [...progress.solved, key] : progress.solved,
    requiredSolved: first && required ? Math.min(progress.requiredTotal, progress.requiredSolved + 1) : progress.requiredSolved,
    completed: progress.completed || result.labCompleted,
  };
}

/** The opened hints of one objective, in order. */
export function hintsOf(progress: LabProgress, key: string): OpenedHint[] {
  return progress.hints.filter((h) => h.key === key).sort((a, b) => a.index - b.index);
}

/** The progress with one more opened hint (kept once, in order). */
export function withHint(progress: LabProgress, hint: OpenedHint): LabProgress {
  if (progress.hints.some((h) => h.key === hint.key && h.index === hint.index)) return progress;
  return { ...progress, hints: [...progress.hints, hint] };
}

/** The progress with a challenge's steps opened. */
export function withSteps(progress: LabProgress, key: string): LabProgress {
  return progress.stepsOpened.includes(key) ? progress : { ...progress, stepsOpened: [...progress.stepsOpened, key] };
}

/** True once the player opened any help (steps, or a hint of the challenge or its steps). */
export function usedHelp(progress: LabProgress, challenge: Challenge): boolean {
  const keys = new Set([challenge.key, ...challenge.steps.map((s) => s.key)]);
  return progress.stepsOpened.includes(challenge.key) || progress.hints.some((h) => keys.has(h.key));
}

/** The challenges open at first: all of a short list, else the first one not solved yet. */
export function initiallyOpen(challenges: Challenge[], solved: string[]): string[] {
  if (challenges.length <= 3) return challenges.map((c) => c.key);
  const next = challenges.find((c) => !solved.includes(c.key));
  return next ? [next.key] : [];
}
