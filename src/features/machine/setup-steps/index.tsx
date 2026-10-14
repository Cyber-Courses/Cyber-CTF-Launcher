"use client";

/**
 * The machine-setup steps, shared by the "Set up this machine" window and the first-run
 * onboarding. Each flow owns its own frame (header style, progress, nav buttons); the step
 * list, titles, bodies and install/test state all live here, so a change shows up in both.
 */

import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SelfTest } from "@/features/machine/self-test";
import { type SystemReport } from "@/lib/tauri";
import { AttackStep } from "@/features/machine/setup-steps/attack-step";
import { EngineStep } from "@/features/machine/setup-steps/engine-step";
import { CmdRow, Outcome, Requirement, Skipped } from "@/features/machine/setup-steps/parts";
import { MachineStep, hasHypervisor, isDockerReady } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { VagrantStep, VmStep } from "@/features/machine/setup-steps/vm-steps";
import { WslStep } from "@/features/machine/setup-steps/wsl-step";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** The body of one setup step (no header, no nav: the flow draws those). */
export function MachineStepBody({ step, report, setup }: { step: MachineStep; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const isWin = report.os === "windows";
  const isMac = report.os === "macos";

  if (step === "pkgmgr") {
    // Homebrew builds from source on Intel Macs (no bottles since macOS 26): say so before the
    // installs that go through it, so a long "Installing…" isn't taken for a hang.
    const intelMac = isMac && report.arch === "x86_64" && <p className="mt-3 text-[0.75rem] text-muted-foreground">{t("machine.body.pkgmgr.intelMac")}</p>;
    if (report.pkgManager.installed) {
      return (
        <>
          <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
            <Requirement ok title={report.pkgManager.name} detail={report.pkgManager.version ?? t("machine.body.installed")} action={null} />
          </div>
          {intelMac}
        </>
      );
    }
    return (
      <>
        {intelMac}
        {isMac && (
          <div className="space-y-2">
            <p className="text-[0.8125rem] text-muted-foreground">{t("machine.body.pkgmgr.macRun")}</p>
            <CmdRow cmd={'/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'} />
            <button onClick={() => openExternal("https://brew.sh")} className="inline-flex items-center gap-1.5 text-[0.75rem] text-jewel-text hover:underline">
              <ExternalLink className="size-3.5" /> brew.sh
            </button>
          </div>
        )}
        {isWin && (
          <div className="flex items-center justify-between gap-3 rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)] px-4 py-3">
            <div>
              <p className="text-[0.8125rem] font-medium text-foreground">{t("machine.body.pkgmgr.winget")}</p>
              <p className="text-[0.75rem] text-muted-foreground">{t("machine.body.pkgmgr.wingetHint")}</p>
            </div>
            <Button variant="primary" size="sm" onClick={() => openExternal("https://apps.microsoft.com/detail/9nblggh4nns1")}>
              <ExternalLink className="size-3.5" /> {t("machine.body.pkgmgr.get")}
            </Button>
          </div>
        )}
        {!isMac && !isWin && (
          <p className="text-[0.8125rem] text-muted-foreground">
            {report.pkgManager.name ? t("machine.body.pkgmgr.linuxNamed", { name: report.pkgManager.name }) : t("machine.body.pkgmgr.linux")}
          </p>
        )}
      </>
    );
  }

  if (step === "virtualization") return <WslStep report={report} setup={setup} />;
  if (step === "docker") return <EngineStep report={report} setup={setup} />;
  if (step === "attack") return <AttackStep report={report} />;
  if (step === "vm") return <VmStep report={report} setup={setup} />;
  if (step === "vagrant") return <VagrantStep report={report} setup={setup} />;

  if (step === "docker-test") {
    return isDockerReady(report) ? (
      <SelfTest
        kind="docker"
        title={t("machine.selfTest.containerTitle")}
        description={t("machine.body.dockerTestDescription")}
        auto={setup.dockerTest === "idle"}
        onResult={setup.setDockerTest}
      />
    ) : (
      <Skipped title={t("machine.body.nothingToTest")} reason={t("machine.body.noEngine")} />
    );
  }

  return hasHypervisor(report) ? (
    <SelfTest
      kind="vm"
      title={t("machine.selfTest.vmTitle")}
      description={t("machine.body.vmTestDescription")}
      // Once per setup: coming back to a passed step shows its run, it doesn't boot another VM.
      auto={setup.vmTest === "idle"}
      onResult={setup.setVmTest}
    />
  ) : (
    <Skipped title={t("machine.body.nothingToTest")} reason={t("machine.body.noHypervisor")} />
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
