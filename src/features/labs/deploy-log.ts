import type { T } from "@/lib/i18n";
import { machineLabel } from "@/lib/format";
import type { DeployStep, StepCount } from "@/lib/tauri/generated";
import type { SaidStep } from "@/lib/tauri/run-events";
import { FETCH_LINES, PREPARE_LINES, STEP_IDS } from "@/lib/tauri";

/**
 * A lab's start-up log as named steps, built from the steps the runner says (its `step` run
 * events), never from a tool's words:
 *  - Docker Compose: get the images, create, start, wait until healthy.
 *  - Vagrant: get the boxes, then one step per machine (dc01, ws01, …), started then set up.
 *  - Terraform: set up Terraform, create the infrastructure, then install and start the lab.
 * Before the runner's first step, the launcher's own: the lab's download, and the preparation of
 * this machine. Each log line goes to the step running when it came.
 */

/** A log line and when it arrived (ms). */
export type Timed = { line: string; at: number };

/** Where a step the runner said stands: running, done, failed, or cut short. */
type SaidState = "running" | "ok" | "fail" | "skipped";

/** A step as the deploy panel shows it: its lines, and, for one the runner said, where it stands
 *  (`said`), when it began and ended, and how far it is (`detail`: "2/5", or what a machine does
 *  now). */
export type Step = {
  id: string;
  label: string;
  rows: Timed[];
  said?: { state: SaidState; from: number; to: number | null; detail: string | null };
};

const startsAny = (starts: readonly string[]) => (l: string) => starts.some((start) => l.startsWith(start));
// A line the launcher prints while fetching/placing the lab, before the runner's steps.
const isDownloadLine = startsAny(FETCH_LINES);
// The launcher's own narration while it readies this machine for a lab's VMs (its memory, QEMU's
// emulation, a previous run's leftovers).
const isPrepLine = startsAny(PREPARE_LINES);

type StepId = (typeof STEP_IDS)[number];
const isStepId = (id: string): id is StepId => (STEP_IDS as readonly string[]).includes(id);

/** A machine's steps (`up`, then `provision`) share the machine's row. */
const MACHINE_STEPS: readonly string[] = ["up", "provision"];

/** The row a step of the runner's goes to: its id and label. */
function rowOf(t: T, { step, label }: DeployStep): { id: string; label: string } {
  if (label && MACHINE_STEPS.includes(step)) return { id: `machine:${label}`, label: machineLabel(label) };
  const name = isStepId(step) ? t(`labs.deploy.phases.${step}`) : step;
  return label ? { id: `${step}:${label}`, label: `${name}: ${label}` } : { id: step, label: name };
}

const SAID: Record<DeployStep["state"], SaidState> = { started: "running", done: "ok", failed: "fail", skipped: "skipped" };

/** How far a step is, as its row says it: "2/5" (or "2" before the total is known). */
const counted = (count: StepCount | null) => (count ? [count.done, count.total].filter((n) => n !== null).join("/") : null);

/** Row `s` as step mark `mark` leaves it. */
function marked(t: T, s: Step, { step, at }: SaidStep): Step {
  const state = SAID[step.state];
  const machine = s.id.startsWith("machine:");
  // A machine's row says what it does while it runs; any other row how far it is.
  const doing = state === "running" && isStepId(step.step) ? t(`labs.deploy.phases.${step.step}`) : null;
  const detail = machine ? doing : (counted(step.count) ?? s.said?.detail ?? null);
  const from = s.said?.from ?? at;
  return { ...s, said: { state, from, to: state === "running" ? null : at, detail } };
}

/** The launcher's own step for `line`, before the runner said one. */
function ownStep(t: T, line: string, last: Step | undefined): { id: string; label: string } {
  if (isDownloadLine(line)) return { id: "download", label: t("labs.deploy.steps.download") };
  if (last && last.id !== "download") return last;
  return { id: "prepare", label: isPrepLine(line) ? t("labs.deploy.steps.prepareMachine") : t("labs.deploy.steps.preparing") };
}

/** Builds the ordered step list from the timed log and the steps the runner said in it. */
export function deriveSteps(t: T, timed: Timed[], marks: readonly SaidStep[] = []): Step[] {
  const steps: Step[] = [];
  // The runner's steps running now, the one started last at the end: a line goes to it.
  let running: string[] = [];
  // The runner's step a line went to last: lines go on there once none runs.
  let current: string | null = null;
  const take = (mark: SaidStep) => {
    const { id, label } = rowOf(t, mark.step);
    const at = steps.findIndex((s) => s.id === id);
    const row = marked(t, at >= 0 ? steps[at] : { id, label, rows: [] }, mark);
    if (at >= 0) steps[at] = row;
    else steps.push(row);
    running = running.filter((r) => r !== id);
    if (row.said?.state === "running") running.push(id);
    current = id;
  };
  let next = 0;
  for (const [i, row] of timed.entries()) {
    while (next < marks.length && marks[next].line <= i) take(marks[next++]);
    const id = running.at(-1) ?? current;
    let s = id ? steps.find((x) => x.id === id) : undefined;
    if (!s) {
      const own = ownStep(t, row.line, steps.at(-1));
      s = steps.find((x) => x.id === own.id);
      if (!s) {
        s = { ...own, rows: [] };
        steps.push(s);
      }
    }
    s.rows.push(row);
  }
  while (next < marks.length) take(marks[next++]);
  return steps;
}
