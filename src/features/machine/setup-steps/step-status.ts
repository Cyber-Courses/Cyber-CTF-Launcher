import { hasHypervisor, isDockerReady, pathSteps, wslState, type MachineStep, type SetupPath } from "@/features/machine/setup-steps/steps";
import { cantRun, localHypervisors, vagrantDrives } from "@/features/machine/hypervisors";
import type { LastTest } from "@/lib/settings";
import type { Provider, SystemReport } from "@/lib/tauri";

// Where this machine stands on each guided-setup step, read from the system report and the last
// self-tests: the setup window opens a path at its first step still to do, so a machine set up
// already is never asked to redo it.

/** The last self-test per lab type. */
export type LastTests = Record<"docker" | "vm", LastTest | null>;

/** What a step's standing reads: the report, the last tests, and the hypervisor labs would use. */
type Facts = {
  report: SystemReport;
  last: LastTests;
  /** The installed hypervisor that can run labs, else the first installed one, if any. */
  hv: ReturnType<typeof localHypervisors>[number] | undefined;
};

/** Whether each step is done on this machine. The attack box has a default: always done. */
const DONE: Record<MachineStep, (f: Facts) => boolean> = {
  pkgmgr: ({ report }) => report.pkgManager.installed,
  virtualization: ({ report }) => wslState(report) === "ready",
  docker: ({ report }) => isDockerReady(report),
  "docker-test": ({ last }) => last.docker?.result === "ok",
  attack: () => true,
  // Installed, and able to run here (no /dev/kvm, not in the kvm group: the Health rows say why).
  vm: ({ report, hv }) => hasHypervisor(report) && !(!!hv && cantRun(hv)),
  vagrant: ({ report, hv }) => (hv ? vagrantDrives(hv) : report.vagrant.installed),
  "vm-test": ({ last }) => last.vm?.result === "ok",
};

/** Whether `step` is done on this machine; `preferred` is the hypervisor chosen in Settings. */
export function stepDone(step: MachineStep, report: SystemReport, last: LastTests, preferred: Provider | null): boolean {
  const installed = localHypervisors(report).filter((p) => p.hypervisor === true);
  // The one labs run on: Settings' choice when it can run here, else one that can run labs
  // (QEMU without KVM can, when libvirt can't), else the first installed.
  const runs = installed.filter((p) => !cantRun(p));
  const hv = runs.find((p) => p.provider === preferred) ?? runs.at(0) ?? installed.at(0);
  return DONE[step]({ report, last, hv });
}

/** How far along a path this machine is: its steps done, and how many there are. */
export function pathProgress(path: SetupPath, report: SystemReport, last: LastTests, preferred: Provider | null): { done: number; total: number } {
  const steps = pathSteps(path, report);
  return { done: steps.filter((s) => stepDone(s, report, last, preferred)).length, total: steps.length };
}

/** The first step of a path this machine has still to do, else none (the path is set up). */
export function firstPending(path: SetupPath, report: SystemReport, last: LastTests, preferred: Provider | null): MachineStep | null {
  return pathSteps(path, report).find((s) => !stepDone(s, report, last, preferred)) ?? null;
}
