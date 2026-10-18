"use client";

/**
 * The machine-setup steps, shared by the "Set up this machine" window and the first-run
 * onboarding. Each flow owns its own frame (header style, progress, nav buttons); the step
 * list, titles, bodies and install/test state all live here, so a change shows up in both.
 */

import { SelfTest } from "@/features/machine/self-test";
import { type SystemReport } from "@/lib/tauri";
import { AttackStep } from "@/features/machine/setup-steps/attack-step";
import { EngineStep } from "@/features/machine/setup-steps/engine-step";
import { Outcome, Skipped } from "@/features/machine/setup-steps/parts";
import { PackageManagerStep } from "@/features/machine/setup-steps/pkgmgr-step";
import { MachineStep, hasHypervisor, isDockerReady } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { VagrantStep, VmStep } from "@/features/machine/setup-steps/vm-steps";
import { WslStep } from "@/features/machine/setup-steps/wsl-step";
import { useT } from "@/lib/i18n";

/** The body of one setup step (no header, no nav: the flow draws those). */
export function MachineStepBody({ step, report, setup }: { step: MachineStep; report: SystemReport; setup: MachineSetupState }) {
  switch (step) {
    case "pkgmgr":
      return <PackageManagerStep report={report} />;
    case "virtualization":
      return <WslStep report={report} setup={setup} />;
    case "docker":
      return <EngineStep report={report} setup={setup} />;
    case "attack":
      return <AttackStep report={report} />;
    case "vm":
      return <VmStep report={report} setup={setup} />;
    case "vagrant":
      return <VagrantStep report={report} setup={setup} />;
    case "docker-test":
      return <DockerTestStep report={report} setup={setup} />;
    case "vm-test":
      return <VmTestStep report={report} setup={setup} />;
  }
}

/** The container self-test, once an engine runs. */
function DockerTestStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  if (!isDockerReady(report)) return <Skipped title={t("machine.body.nothingToTest")} reason={t("machine.body.noEngine")} />;
  return (
    <SelfTest
      kind="docker"
      title={t("machine.selfTest.containerTitle")}
      description={t("machine.body.dockerTestDescription")}
      auto={setup.dockerTest === "idle"}
      onResult={setup.setDockerTest}
    />
  );
}

/** The VM self-test, once a hypervisor is installed. */
function VmTestStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  if (!hasHypervisor(report)) return <Skipped title={t("machine.body.nothingToTest")} reason={t("machine.body.noHypervisor")} />;
  return (
    <SelfTest
      kind="vm"
      title={t("machine.selfTest.vmTitle")}
      description={t("machine.body.vmTestDescription")}
      // Once per setup: coming back to a passed step shows its run, it doesn't boot another VM.
      auto={setup.vmTest === "idle"}
      onResult={setup.setVmTest}
    />
  );
}

/** End-of-setup summary: what this machine can run, from the test results. */
export function SetupOutcome({ report, setup }: { report: SystemReport | null; setup: MachineSetupState }) {
  const t = useT();
  const { dockerTest, vmTest } = setup;
  return (
    <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      <Outcome
        title={t("machine.body.outcome.containerLabs")}
        ok={dockerTest === "ok"}
        detail={
          dockerTest === "ok"
            ? t("machine.body.outcome.ready")
            : dockerTest === "fail"
              ? t("machine.body.outcome.failed")
              : isDockerReady(report)
                ? t("machine.body.outcome.engineRunning")
                : t("machine.body.outcome.setUpEngine")
        }
      />
      <Outcome
        title={t("machine.body.outcome.vmLabs")}
        ok={vmTest === "ok"}
        detail={
          vmTest === "ok"
            ? t("machine.body.outcome.ready")
            : vmTest === "fail"
              ? t("machine.body.outcome.failed")
              : hasHypervisor(report)
                ? t("machine.body.outcome.notTested")
                : t("machine.body.outcome.installHypervisor")
        }
      />
    </div>
  );
}

// The setup flow's public surface (machine setup window, onboarding, Machine and Servers pages).
export { canContinue, machineSteps, nextLabel, nextVariant, stepMeta, type MachineStep } from "@/features/machine/setup-steps/steps";
export { useMachineSetup, type MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
export { EngineMark } from "@/features/machine/setup-steps/engine-step";
export { engineName } from "@/features/machine/setup-steps/engines";
export { Requirement } from "@/features/machine/setup-steps/parts";
