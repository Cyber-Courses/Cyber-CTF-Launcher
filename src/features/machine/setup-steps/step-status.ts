import { hasHypervisor, isDockerReady, pathSteps, setupHypervisor, wslState, type MachineStep, type SetupPath } from "@/features/machine/setup-steps/steps";
import { cantRun } from "@/features/machine/hypervisors";
import { vagrantActions } from "@/features/machine/setup-steps/plan";
import type { LastTest } from "@/lib/settings";
import type { SystemReport } from "@/lib/tauri";

// Where this machine stands on each guided-setup step, read from the system report and the last
// self-tests: the setup window opens a path at its first step still to do, so a machine set up
// already is never asked to redo it.

/** The last self-test per lab type. */
export type LastTests = Record<"docker" | "vm", LastTest | null>;

/** What a step's standing reads: the report, the last tests, and the hypervisor the setup works on. */
type Facts = {
  report: SystemReport;
  last: LastTests;
  /** The hypervisor the steps' bodies show (`setupHypervisor`), if any is offered. */
  hv: ReturnType<typeof setupHypervisor>;
};

/** Whether each step is done on this machine. The attack box has a default: always done. */
const DONE: Record<MachineStep, (f: Facts) => boolean> = {
  pkgmgr: ({ report }) => report.pkgManager.installed,
  virtualization: ({ report }) => wslState(report) === "ready",
  docker: ({ report }) => isDockerReady(report),
  "docker-test": ({ last }) => last.docker?.result === "ok",
  attack: () => true,
  // Installed, and able to run here (no /dev/kvm, not in the kvm group: the Health rows say why).
  vm: ({ report, hv }) => hasHypervisor(report) && !(hv?.hypervisor === true && cantRun(report, hv)),
  // Vagrant has nothing to drive until that hypervisor is installed: Vagrant itself, then.
  vagrant: ({ report, hv }) => (hv?.hypervisor === true ? vagrantActions(report, hv.provider).length === 0 : report.vagrant.installed),
  "vm-test": ({ last }) => last.vm?.result === "ok",
};

/** Whether `step` is done on this machine, for the hypervisor the setup works on (`picked`: the
 *  flow's pick, null before one; see `setupHypervisor`), so a step reads done for the hypervisor
 *  its body shows. */
export function stepDone(step: MachineStep, report: SystemReport, last: LastTests, picked: string | null): boolean {
  return DONE[step]({ report, last, hv: setupHypervisor(report, picked) });
}

/** How far along a path this machine is: its steps done, and how many there are. */
export function pathProgress(path: SetupPath, report: SystemReport, last: LastTests, picked: string | null): { done: number; total: number } {
  const steps = pathSteps(path, report);
  return { done: steps.filter((s) => stepDone(s, report, last, picked)).length, total: steps.length };
}

/** The first step of a path this machine has still to do, else none (the path is set up). */
export function firstPending(path: SetupPath, report: SystemReport, last: LastTests, picked: string | null): MachineStep | null {
  return pathSteps(path, report).find((s) => !stepDone(s, report, last, picked)) ?? null;
}
