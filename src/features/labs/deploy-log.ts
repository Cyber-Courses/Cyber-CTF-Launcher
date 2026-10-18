import type { T } from "@/lib/i18n";

/**
 * A lab's start-up log read as named steps. Which steps appear depends on the target the
 * launcher is driving, detected from the output itself:
 *  - Docker Compose: pull images, create, start, wait until healthy.
 *  - Vagrant (a VM on this machine, or an ESXi host): one step per machine (dc01, ws01, …), each
 *    showing its live action (importing the box, booting, provisioning).
 *  - Terraform (a cloud account or Proxmox): set up Terraform, create the infrastructure, then
 *    install and start the lab on the host.
 */

/** A log line and when it arrived (ms). */
export type Timed = { line: string; at: number };
export type Step = { id: string; label: string; rows: Timed[] };

// Docker Compose phases, matched line by line.
type Phase = { id: "images" | "create" | "start" | "health"; match: (line: string) => boolean };
const DOCKER_PHASES: Phase[] = [
  { id: "images", match: (l) => /\bImage\b.*\b(Pulling|Pulled|Building|Built)\b|^\s*(Pulling|Building)\b/.test(l) },
  { id: "create", match: (l) => /\b(Network|Volume|Container)\b.*\b(Creating|Created)\b/.test(l) },
  { id: "start", match: (l) => /\bContainer\b.*\b(Starting|Started)\b/.test(l) },
  { id: "health", match: (l) => /\bContainer\b.*\b(Waiting|Healthy|Exited)\b/.test(l) },
];

/** Which target's output this is, inferred from the lines seen so far. */
export function detectTarget(text: string): "vagrant" | "terraform" | "docker" | "unknown" {
  if (/^\s*==>\s*\S+:/m.test(text) || /Bringing machine '.*' up/.test(text)) return "vagrant";
  if (/Initializing the backend|Terraform (has been|will perform)|^\s*[\w.[\]"-]+: (Creating|Creation complete|Still creating)/m.test(text)) return "terraform";
  if (/\bContainer\b.*\b(Creating|Started|Running)\b|^\s*Pulling\s|\bNetwork\b.*\bCreat/m.test(text)) return "docker";
  return "unknown";
}

// A line the launcher prints while fetching/placing the lab, before the target's own output.
const isDownloadLine = (l: string) => /^(Downloading|Cloning|Lab installed|Fetching)\b|^Running (on server host|in a .* VM)/.test(l);
// The launcher's own narration during a local VM start (recovery, provider choice).
const isPrepLine = (l: string) => /left VM state behind|Clearing leftover|different target|Recovering|Preparing the lab/.test(l);

const machineLabel = (name: string) => name.replace(/^isoloom-/, "");

/** The step of the machine a Vagrant failure names ("…while executing the action on the
 *  'isoloom-controller' machine"), if it names one: that step failed, not necessarily the last
 *  one shown (another machine's lines can come last, as in a parallel `vagrant up`). */
export function failedMachineStep(failure: string): string | null {
  const m = failure.match(/executing the action on the '([A-Za-z0-9_.-]+)'/);
  return m ? `m:${m[1]}` : null;
}

/** Which step a line goes to: its id and label. */
type StepKey = { id: string; label: string };
/** Places each line in a step, for one target; it keeps what earlier lines set (the machine, the
 *  Docker phase). `last` is the step the previous line went to. */
type Stepper = (line: string, last: Step | undefined) => StepKey;

/** Vagrant: one step per machine, from the first `==> name:` line on; before it, the start. */
function vagrantStepper(t: T): Stepper {
  let machine: string | null = null;
  return (l) => {
    const m = /^\s*==>\s*([A-Za-z0-9_.-]+):/.exec(l);
    if (m) machine = m[1];
    if (machine) return { id: `m:${machine}`, label: machineLabel(machine) };
    return { id: "prepare", label: isPrepLine(l) ? t("labs.deploy.steps.prepareMachine") : t("labs.deploy.steps.start") };
  };
}

/** Terraform: set up, create the infrastructure, then the lab on its host; any other line stays
 *  in the step before it. */
function terraformStepper(t: T): Stepper {
  return (l, last) => {
    if (/Initializing|terraform init|Installing|Finding .* versions|Reusing previous/.test(l)) return { id: "tf-init", label: t("labs.deploy.steps.tfInit") };
    if (/Creating\.\.\.|Creation complete|Still creating|Destroying|Apply complete|Plan:|will perform|Modif/.test(l))
      return { id: "tf-apply", label: t("labs.deploy.steps.tfApply") };
    if (/Waiting for the lab host|running:|install Docker|cloud-init|bootstrap|is ready|ready/i.test(l))
      return { id: "tf-ready", label: t("labs.deploy.steps.tfReady") };
    return { id: last?.id ?? "tf-init", label: last?.label ?? t("labs.deploy.steps.tfInit") };
  };
}

/** Docker Compose: its phases, each from the first line that starts it; before them, preparing. */
function dockerStepper(t: T): Stepper {
  let phase: Phase | null = null;
  return (l) => {
    phase = DOCKER_PHASES.find((ph) => ph.match(l)) ?? phase;
    if (phase) return { id: phase.id, label: t(`labs.deploy.phases.${phase.id}`) };
    return { id: "prepare", label: t("labs.deploy.steps.prepareLab") };
  };
}

/** An unknown target: every line in the step before it (preparing, at first). */
const otherStepper =
  (t: T): Stepper =>
  (_l, last) => ({ id: last?.id ?? "prepare", label: last?.label ?? t("labs.deploy.steps.preparing") });

const STEPPERS: Record<ReturnType<typeof detectTarget>, (t: T) => Stepper> = {
  vagrant: vagrantStepper,
  terraform: terraformStepper,
  docker: dockerStepper,
  unknown: otherStepper,
};

/** Builds the ordered step list from the timed log, per the detected target. */
export function deriveSteps(t: T, timed: Timed[]): Step[] {
  const steps: Step[] = [];
  const stepOf = STEPPERS[detectTarget(timed.map((row) => row.line).join("\n"))](t);
  for (const row of timed) {
    const { id, label } = isDownloadLine(row.line) ? { id: "download", label: t("labs.deploy.steps.download") } : stepOf(row.line, steps.at(-1));
    let s = steps.find((x) => x.id === id);
    if (!s) {
      s = { id, label, rows: [] };
      steps.push(s);
    }
    s.rows.push(row);
  }
  return steps;
}
