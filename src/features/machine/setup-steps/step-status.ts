import { hasHypervisor, isDockerReady, pathSteps, wslState, type MachineStep, type SetupPath } from "@/features/machine/setup-steps/steps";
import { engineName } from "@/features/machine/setup-steps/engines";
import { cantRun, providerLabel, usableHypervisors } from "@/features/machine/hypervisors";
import { ATTACK_PRESETS, getAttackImage, type LastTest } from "@/lib/settings";
import type { Provider, ProviderStatus, SystemReport } from "@/lib/tauri";
import { translate, type MessageKey, type Vars } from "@/lib/i18n";

// Where this machine stands on each guided-setup step, read from the system report and the last
// self-tests: the Machine page's checklist draws a row from it, and the setup window opens a
// path at its first step still to do, so a machine set up already is never asked to redo it.

type Tr = (key: MessageKey, vars?: Vars) => string;

/** One step's standing: done or not, optional (VM and cloud labs run elsewhere too), and a short
 *  mono note (the engine's name, "not installed", "passed"). */
export type Line = { done: boolean; optional?: boolean; meta: string };

/** The last self-test per lab type. */
export type LastTests = Record<"docker" | "vm", LastTest | null>;

/** What every step's line reads: the report, the last tests, and the hypervisor labs would use. */
type Facts = {
  report: SystemReport;
  last: LastTests;
  t: Tr;
  /** The installed hypervisor that can run labs, else the first installed one. */
  hv: ProviderStatus | undefined;
  /** It is installed yet can't run here: not done, as the Health tab says. */
  blocked: boolean;
};

const tested = (t: Tr, r: LastTest | null) => {
  if (!r) return t("machine.checklist.notTested");
  return r.result === "ok" ? t("machine.checklist.passed") : t("machine.checklist.failed");
};

function wslLine({ report, t }: Facts): Line {
  const wsl = wslState(report);
  if (wsl === "ready") return { done: true, meta: "WSL 2" };
  if (wsl === "needsReboot") return { done: false, meta: t("machine.checklist.restartNeeded") };
  if (wsl === "missing") return { done: false, meta: t("machine.checklist.notInstalled") };
  return { done: false, meta: t("machine.checklist.notChecked") };
}

function dockerLine({ report, t }: Facts): Line {
  if (isDockerReady(report)) return { done: true, meta: report.dockerEngine ? engineName(report.dockerEngine) : t("machine.checklist.running") };
  return { done: false, meta: report.docker.installed ? t("machine.checklist.stopped") : t("machine.checklist.notInstalled") };
}

function vagrantLine({ report, t, hv }: Facts): Line {
  const optional = !hasHypervisor(report);
  const missingPlugin = hv?.plugin && !hv.pluginInstalled ? hv.plugin : null;
  if (!report.vagrant.installed) return { done: false, optional, meta: t("machine.checklist.notInstalled") };
  if (missingPlugin) return { done: false, optional, meta: t("machine.checklist.pluginMissing", { plugin: missingPlugin }) };
  return { done: true, optional, meta: t("machine.checklist.installed") };
}

/** Where this machine stands on each guided-setup step. The steps themselves run in the setup window. */
const LINES: Record<MachineStep, (f: Facts) => Line> = {
  pkgmgr: ({ report, t }) => ({
    done: report.pkgManager.installed,
    meta: report.pkgManager.installed ? report.pkgManager.name : t("machine.checklist.notInstalled"),
  }),
  virtualization: wslLine,
  docker: dockerLine,
  "docker-test": ({ last, t }) => ({ done: last.docker?.result === "ok", meta: tested(t, last.docker) }),
  attack: () => {
    const image = getAttackImage();
    return { done: true, meta: ATTACK_PRESETS.find((p) => p.image === image)?.label ?? image };
  },
  vm: ({ report, t, hv, blocked }) => ({
    done: hasHypervisor(report) && !blocked,
    optional: true,
    meta: hv ? `${providerLabel(hv, report.os)}${blocked ? t("machine.checklist.cantRunHere") : ""}` : t("machine.checklist.optional"),
  }),
  vagrant: vagrantLine,
  "vm-test": ({ report, last, t }) => ({ done: last.vm?.result === "ok", optional: !hasHypervisor(report), meta: tested(t, last.vm) }),
  // Terraform creates cloud labs from this machine; the account itself is connected on the Cloud page.
  cloud: ({ report, t }) => ({
    done: report.terraform.installed,
    optional: true,
    meta: report.terraform.installed ? (report.terraform.version ?? t("machine.checklist.installed")) : t("machine.checklist.notInstalled"),
  }),
};

/** Where this machine stands on `step`; `preferred` is the hypervisor chosen in Settings. */
export function stepLine(step: MachineStep, report: SystemReport, last: LastTests, preferred: Provider | null, t: Tr = translate): Line {
  const installed = usableHypervisors(report).filter((p) => p.hypervisor === true);
  // The one labs run on: Settings' choice when it can run here, else one that can run labs
  // (QEMU without KVM can, when libvirt can't), else the first installed.
  const runs = installed.filter((p) => !cantRun(p, report));
  const hv = runs.find((p) => p.provider === preferred) ?? runs.at(0) ?? installed.at(0);
  return LINES[step]({ report, last, t, hv, blocked: !!hv && cantRun(hv, report) });
}

/** How far along a path this machine is: its steps done, and how many there are. */
export function pathProgress(path: SetupPath, report: SystemReport, last: LastTests, preferred: Provider | null): { done: number; total: number } {
  const steps = pathSteps(path, report);
  return { done: steps.filter((s) => stepLine(s, report, last, preferred).done).length, total: steps.length };
}

/** The first step of a path this machine has still to do, else none (the path is set up). */
export function firstPending(path: SetupPath, report: SystemReport, last: LastTests, preferred: Provider | null): MachineStep | null {
  return pathSteps(path, report).find((s) => !stepLine(s, report, last, preferred).done) ?? null;
}
