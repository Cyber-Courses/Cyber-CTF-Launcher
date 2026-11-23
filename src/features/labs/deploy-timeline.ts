import type { T } from "@/lib/i18n";
import { deriveSteps, type Step, type Timed } from "@/features/labs/deploy-log";
import type { SaidStep } from "@/lib/tauri/run-events";
import type { StepState } from "@/features/labs/deploy-step-parts";

/** An operation's words (busy, its one step, done). */
export type OperationText = { busy: string; step: string; done: string };

/** The steps of `timed` and the steps the runner said in it (`marks`): the start's (or the
 *  operation's), what came after Ready as a step of its own, the step running now, and the one
 *  that failed. */
export function stepsOf(
  t: T,
  timed: Timed[],
  marks: readonly SaidStep[],
  { done, failed, op, operation }: { done: Timed | undefined; failed: Timed | undefined; op: OperationText | undefined; operation: string },
) {
  // What came after Ready (the attack box added to the running lab, a change to it) is a step of
  // its own below Ready, not more of the start's steps: they would read as running again under a
  // green Ready.
  const readyAt = done ? timed.indexOf(done) : -1;
  const startRows = readyAt >= 0 ? timed.slice(0, readyAt + 1) : timed;
  const startMarks = readyAt >= 0 ? marks.filter((m) => m.line <= readyAt) : marks;
  const afterRows = readyAt >= 0 ? timed.slice(readyAt + 1) : [];
  const opSteps = timed.length > 0 && op ? [{ id: operation, label: op.step, rows: timed }] : [];
  const steps: Step[] = op ? opSteps : deriveSteps(t, startRows, startMarks);
  const after: Step | null = !op && afterRows.length > 0 ? { id: "after-ready", label: afterReadyLabel(t, afterRows), rows: afterRows } : null;
  // The step holding the latest line, not the one made last: a machine's step goes on while
  // another's begins.
  const latest = startRows.at(-1);
  const lastSeen = after ? after.id : (steps.find((s) => latest !== undefined && s.rows.at(-1) === latest) ?? steps.at(-1))?.id;
  return { steps, after, lastSeen, failedStep: failedStepOf(steps, after, failed, failed ? timed.indexOf(failed) > readyAt : false, lastSeen) };
}

/** The step a failure belongs to: the after-Ready step for one after Ready, else the step the
 *  runner said failed, else the step running when it came. */
function failedStepOf(steps: Step[], after: Step | null, failed: Timed | undefined, afterReady: boolean, lastSeen: string | undefined) {
  if (!failed) return null;
  if (after && afterReady) return after.id;
  return steps.find((s) => s.said?.state === "fail")?.id ?? lastSeen;
}

/** What the lines after Ready are: the attack box being added, or another change to the lab. */
function afterReadyLabel(t: T, rows: Timed[]): string {
  return rows.some((row) => /attack/i.test(row.line)) ? t("labs.deploy.steps.attackBox") : t("labs.deploy.steps.afterReady");
}

/** What a step's state and span are read from: the run's steps, how it ended, when it began, the
 *  step that failed and the one holding the latest line. */
type StepTimeline = {
  steps: Step[];
  done: Timed | undefined;
  failed: Timed | undefined;
  start: number | undefined;
  failedStep: string | null | undefined;
  lastSeen: string | undefined;
};

/** When a step began: when the runner said it, else its first line. */
const firstAt = (step: Step) => step.said?.from ?? step.rows.at(0)?.at ?? 0;

/** When a step's last line came (when it began, for one with none). */
const lastAt = (step: Step) => step.rows.at(-1)?.at ?? firstAt(step);

/** Where step `p` stands: the one the failure is of failed; one the runner said stands as it
 *  said (running only while the run is); the launcher's own runs while it holds the latest line. */
export function stateOf(p: Step, tl: StepTimeline, busy: boolean): StepState {
  if (p.id === tl.failedStep) return "fail";
  const live = busy && !tl.done;
  switch (p.said?.state) {
    case "skipped":
      return "pending";
    case "running":
      return live ? "running" : "ok";
    case "ok":
    case "fail":
      return "ok";
    default:
      return p.id === tl.lastSeen && live ? "running" : "ok";
  }
}

/** When step `p` (the `i`th) began and ended (null while it runs): the runner's own times for one
 *  it said; else from its first line (the run's start, for the first step) to the next step's. */
export function spanOf(p: Step, i: number, tl: StepTimeline, state: StepState): { from: number; to: number | null } {
  const { steps, done, failed } = tl;
  const from = i === 0 && tl.start !== undefined ? Math.min(tl.start, firstAt(p)) : firstAt(p);
  if (state === "running") return { from, to: null };
  const end = done?.at ?? failed?.at ?? lastAt(p);
  if (p.said) return { from, to: p.said.to ?? end };
  const next = steps.at(i + 1);
  return { from, to: next ? firstAt(next) : end };
}
