import type { T } from "@/lib/i18n";
import { deriveSteps, failedMachineStep, type Step, type Timed } from "@/features/labs/deploy-log";

/** An operation's words (busy, its one step, done). */
export type OperationText = { busy: string; step: string; done: string };

/** The steps of `timed`: the start's (or the operation's), what came after Ready as a step of its
 *  own, the step running now, and the one that failed. */
export function stepsOf(
  t: T,
  timed: Timed[],
  { done, failed, op, operation }: { done: Timed | undefined; failed: Timed | undefined; op: OperationText | undefined; operation: string },
) {
  // What came after Ready (the attack box added to the running lab, a change to it) is a step of
  // its own below Ready, not more of the start's steps: their Docker phase or machine would read
  // as running again under a green Ready.
  const readyAt = done ? timed.indexOf(done) : -1;
  const startRows = readyAt >= 0 ? timed.slice(0, readyAt + 1) : timed;
  const afterRows = readyAt >= 0 ? timed.slice(readyAt + 1) : [];
  const opSteps = timed.length > 0 && op ? [{ id: operation, label: op.step, rows: timed }] : [];
  const steps: Step[] = op ? opSteps : deriveSteps(t, startRows);
  const after: Step | null = !op && afterRows.length > 0 ? { id: "after-ready", label: afterReadyLabel(t, afterRows), rows: afterRows } : null;
  // The step holding the latest line, not the one made last: a Docker VM's machine step goes on
  // after its images step, so the images step is made after it but ends before it.
  const latest = startRows.at(-1);
  const lastSeen = after ? after.id : (steps.find((s) => s.rows.at(-1) === latest) ?? steps.at(-1))?.id;
  return { steps, after, lastSeen, failedStep: failedStepOf(steps, after, failed, failed ? timed.indexOf(failed) > readyAt : false, lastSeen) };
}

/** The step a failure belongs to: the after-Ready step for one after Ready, else the machine it
 *  names, else the step running when it came. */
function failedStepOf(steps: Step[], after: Step | null, failed: Timed | undefined, afterReady: boolean, lastSeen: string | undefined) {
  if (!failed) return null;
  if (after && afterReady) return after.id;
  const named = failedMachineStep(failed.line);
  return named && steps.some((s) => s.id === named) ? named : lastSeen;
}

/** What the lines after Ready are: the attack box being added, or another change to the lab. */
function afterReadyLabel(t: T, rows: Timed[]): string {
  return rows.some((row) => /attack/i.test(row.line)) ? t("labs.deploy.steps.attackBox") : t("labs.deploy.steps.afterReady");
}
