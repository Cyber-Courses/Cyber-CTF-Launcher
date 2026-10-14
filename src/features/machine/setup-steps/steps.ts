import { Container, Cpu, Crosshair, FlaskConical, Package, Server, type LucideIcon } from "lucide-react";
import { type SystemReport, type WslState } from "@/lib/tauri";
import { translate, type MessageKey, type Vars } from "@/lib/i18n";
import { cantRun, INSTALLABLE, usableHypervisors } from "@/features/machine/hypervisors";
import { ENGINES, Engine } from "@/features/machine/setup-steps/engines";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";

export type MachineStep = "pkgmgr" | "virtualization" | "docker" | "docker-test" | "attack" | "vm" | "vagrant" | "vm-test";

/** Translates a message: `useT()`'s `t` in a component (re-renders on a language change),
 *  `translate` (the current language) elsewhere. */
type Tr = (key: MessageKey, vars?: Vars) => string;

/** The steps this machine needs, in order. OS-aware: Windows gets the WSL step. */
export function machineSteps(report: SystemReport | null): MachineStep[] {
  return ["pkgmgr", ...(report?.os === "windows" ? (["virtualization"] as const) : []), "docker", "docker-test", "attack", "vm", "vagrant", "vm-test"];
}

export const isDockerReady = (r: SystemReport | null) => !!r && r.docker.installed && r.dockerRunning;

/** Where WSL 2 stands (Windows): the machine check's answer, and a restart pending once
 *  "Turn on WSL 2" finished in this flow. A running Docker engine is proof enough whatever the
 *  check could tell (#224): onboarding and the Machine checklist both read it from here, so they
 *  never disagree, and a working machine is never offered an install and a restart it doesn't need. */
export function wslState(report: SystemReport | null, setup?: Pick<MachineSetupState, "wslTurnedOn">): WslState {
  const state = report?.wsl?.state ?? "unknown";
  if (state !== "ready" && isDockerReady(report)) return "ready";
  return state !== "ready" && setup?.wslTurnedOn ? "needsReboot" : state;
}
export const hasHypervisor = (r: SystemReport | null) => !!r && usableHypervisors(r).some((p) => p.hypervisor === true);

export function stepMeta(step: MachineStep, report: SystemReport | null, t: Tr = translate): { icon: LucideIcon; title: string; description: string } {
  const pm = report?.pkgManager.name ?? t("machine.steps.aPackageManager");
  // macOS asks for a few things along the way; say so before the step that triggers each prompt.
  const mac = report?.os === "macos";
  const withMac = (text: string, headsUp: MessageKey) => (mac ? `${text} ${t(headsUp)}` : text);
  switch (step) {
    case "pkgmgr":
      return {
        icon: Package,
        title: t("machine.steps.pkgmgr.title"),
        description: t("machine.steps.pkgmgr.description", { pm }),
      };
    case "virtualization":
      return {
        icon: Cpu,
        title: t("machine.steps.virtualization.title"),
        description: t("machine.steps.virtualization.description"),
      };
    case "docker":
      return {
        icon: Container,
        title: t("machine.steps.docker.title"),
        description: report?.os === "windows" ? t("machine.steps.docker.descriptionWindows") : t("machine.steps.docker.description"),
      };
    case "attack":
      return {
        icon: Crosshair,
        title: t("machine.steps.attack.title"),
        description: withMac(t("machine.steps.attack.description"), "machine.steps.macTerminal"),
      };
    case "vm":
      return {
        icon: Server,
        title: t("machine.steps.vm.title"),
        description: withMac(t("machine.steps.vm.description"), "machine.steps.macInstallerVm"),
      };
    case "vagrant":
      return {
        icon: Package,
        title: t("machine.steps.vagrant.title"),
        description: withMac(t("machine.steps.vagrant.description"), "machine.steps.macInstaller"),
      };
    case "docker-test":
      return {
        icon: FlaskConical,
        title: t("machine.steps.dockerTest.title"),
        description: withMac(t("machine.steps.dockerTest.description"), "machine.steps.macLocalNetwork"),
      };
    case "vm-test":
      return {
        icon: FlaskConical,
        title: t("machine.steps.vmTest.title"),
        description: withMac(t("machine.steps.vmTest.description"), "machine.steps.macLocalNetwork"),
      };
  }
}

/** Install + test state for the setup steps. One per flow, passed to every step body.
 *  Starts each test's download in the background as soon as the machine can run it. */

/** What the flow's forward button does on this step: continue, or move on without it done. */
type NextKind = "continue" | "restartLater" | "skip" | "done";

function nextKind(step: MachineStep, report: SystemReport | null, setup?: Pick<MachineSetupState, "wslTurnedOn">): NextKind {
  if (step === "virtualization") {
    // Found on: nothing to confirm. Waiting for a restart: the step's own button restarts, this
    // one moves on without. Missing: skip. Only when the check can't tell does the player say.
    const state = wslState(report, setup);
    if (state === "ready") return "continue";
    if (state === "needsReboot") return "restartLater";
    if (state === "missing") return "skip";
    return "done";
  }
  // A hypervisor is optional (VM labs can run on a server): with none set up, the VM steps
  // can be passed by.
  if ((step === "vm" || step === "vagrant" || step === "vm-test") && !hasHypervisor(report)) return "skip";
  return "continue";
}

/** Label for the flow's forward button on this step. */
export function nextLabel(step: MachineStep, report: SystemReport | null, t: Tr = translate, setup?: Pick<MachineSetupState, "wslTurnedOn">): string {
  return t(`machine.steps.next.${nextKind(step, report, setup)}`);
}

/**
 * The forward button's look: the jewel only for a Continue once the step is set up. Skip for
 * now, I've done this and I'll restart later are ways past a step, not what we nudge players
 * to, and while the step's own action (Install, Turn on WSL 2) is the thing to do, that action
 * is the view's one jewel (docs/DESIGN.md).
 */
export function nextVariant(step: MachineStep, report: SystemReport | null, setup: MachineSetupState): "primary" | "secondary" {
  return nextKind(step, report, setup) === "continue" && canContinue(step, report, setup) ? "primary" : "secondary";
}

/** The engine the player picked, else the one in use (it can be reported after the step
 *  first shows), else the recommended one for this OS. */
export function chosenEngine(report: SystemReport, setup: MachineSetupState): Engine {
  const engines = ENGINES.filter((e) => e.os.includes(report.os));
  const recommended = report.os === "macos" || report.os === "windows" ? "docker-desktop" : "docker-engine";
  return engines.find((e) => e.id === (setup.engine ?? report.dockerEngine ?? recommended)) ?? engines[0];
}

/** The hypervisor the player picked, else an installed one, else the first Cyber CTF can install. */
export function chosenHypervisor(report: SystemReport, setup: MachineSetupState) {
  const hypervisors = usableHypervisors(report);
  // An installed one that can run labs first (QEMU without KVM can, when libvirt can't).
  const fallback =
    hypervisors.find((p) => p.hypervisor === true && !cantRun(p, report)) ??
    hypervisors.find((p) => p.hypervisor === true) ??
    hypervisors.find((p) => INSTALLABLE[p.provider]) ??
    hypervisors[0];
  return hypervisors.find((p) => p.provider === (setup.hypervisor ?? fallback?.provider)) ?? fallback;
}

/** The flow moves on only once the option picked on this step is actually set up. */
export function canContinue(step: MachineStep, report: SystemReport | null, setup: MachineSetupState): boolean {
  if (!report) return false;
  if (step === "pkgmgr") return report.pkgManager.installed;
  if (step === "docker") {
    if (!isDockerReady(report)) return false;
    // A working engine this step has no tile for (Podman) is fine unless another was picked.
    const known = ENGINES.some((e) => e.id === report.dockerEngine);
    return report.dockerEngine === chosenEngine(report, setup).id || (!known && !setup.engine);
  }
  // A hypervisor is optional, as the step says (VM labs can go to a Server, and one can be added
  // later from the Machine page): the flow never waits on one.
  if (step === "vm") return true;
  if (step === "vagrant") {
    const choice = chosenHypervisor(report, setup);
    // No local hypervisor set up (VM labs go to a Server): nothing for Vagrant to drive here.
    if (!choice || choice.hypervisor !== true) return true;
    // Vagrant, and the plugin that lets it drive the chosen hypervisor.
    return report.vagrant.installed && (!choice.plugin || choice.pluginInstalled);
  }
  return true;
}
