"use client";

import { Container } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TypeIcon } from "@/components/ui/type-icon";
import type { Tone } from "@/components/ui/status-pill";
import { EngineMark, engineName } from "@/features/machine/setup-steps";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { providerLabel } from "@/features/machine/hypervisors";
import { LabTypeRow } from "@/features/machine/machine-parts";
import { dockerState, type DockerState, type VmState, type vmSetup } from "@/features/machine/machine-status";
import { FixButton, TestButton, TestLine, lowFor, useFreeHint, type RowProps } from "@/features/machine/lab-row-parts";
import type { SystemReport } from "@/lib/tauri";
import type { LastTest } from "@/lib/settings";
import { useT, type MessageKey } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

const DOCKER_STATUS: Record<DockerState, MessageKey> = {
  ready: "machine.screen.status.ready",
  testFailed: "machine.screen.status.testFailed",
  denied: "machine.screen.status.noPermission",
  stopped: "machine.screen.status.engineStopped",
  missing: "machine.screen.status.needsSetup",
};

const VM_STATUS: Record<VmState, MessageKey> = {
  notApplicable: "machine.screen.status.notOnThisMachine",
  ready: "machine.screen.status.ready",
  testFailed: "machine.screen.status.testFailed",
  blocked: "machine.screen.status.needsSetup",
  vagrantMissing: "machine.screen.status.needsSetup",
  needHypervisor: "machine.screen.status.needsSetup",
};

/** What a row's tone is once its lab type runs here: its last test passed or failed. */
const TESTED_TONE = { ready: "ok", testFailed: "fail" } as const;

/** Container labs on this machine: the engine, its last test, and the fix or the test. */
export function DockerLabRow({ report, last, now, memFree, testing, onToggleTest, onTestDone, onFix }: RowProps) {
  const t = useT();
  const state = dockerState(report, last);
  const ready = state === "ready" || state === "testFailed";
  const freeHint = useFreeHint("docker", memFree);
  return (
    <LabTypeRow
      kind="docker"
      icon={
        ready && report.dockerEngine ? (
          <EngineMark id={report.dockerEngine} />
        ) : (
          <TypeIcon>
            <Container className="size-4" />
          </TypeIcon>
        )
      }
      title={t("machine.screen.containerLabs")}
      tone={ready ? TESTED_TONE[state] : "warn"}
      status={t(DOCKER_STATUS[state])}
      detail={<DockerDetail report={report} state={state} last={last} now={now} />}
      hint={ready ? freeHint : undefined}
      actions={ready ? <TestButton testing={testing} onClick={onToggleTest} /> : state !== "denied" && <FixButton step="docker" onFix={onFix} />}
      testing={testing}
      onTestDone={onTestDone}
    />
  );
}

/** The engine and its last test once it runs, else why it doesn't. */
function DockerDetail({ report, state, last, now }: { report: SystemReport; state: DockerState; last: LastTest | null; now: number }) {
  const t = useT();
  if (state === "ready" || state === "testFailed")
    return (
      <>
        {report.dockerEngine ? engineName(report.dockerEngine) : "Docker"} · <TestLine test={last} now={now} />
      </>
    );
  if (state === "denied") return report.dockerDeniedHint ? describeError(report.dockerDeniedHint, t) : t("machine.screen.docker.denied");
  if (state === "stopped") return t("machine.screen.docker.stopped");
  // The engines this OS has (OrbStack is macOS-only; Docker Engine is Linux's own).
  if (report.os === "linux") return t("machine.screen.docker.noEngineLinux");
  return report.os === "macos" ? t("machine.screen.docker.noEngineMacos") : t("machine.screen.docker.noEngineWindows");
}

type Vm = ReturnType<typeof vmSetup>;

/** VM labs on this machine: the hypervisor, its last test, and the fix, the test or a server. */
export function VmLabRow({ vm, onUseServer, ...row }: RowProps & { vm: Vm; onUseServer: () => void }) {
  const t = useT();
  const { state, provider } = vm;
  const ready = state === "ready" || state === "testFailed";
  const tone: Tone = state === "notApplicable" ? "muted" : ready ? TESTED_TONE[state] : "warn";
  const freeHint = useFreeHint("vm", row.memFree);
  return (
    <LabTypeRow
      kind="vm"
      icon={<HypervisorLogo provider={provider?.provider} />}
      title={t("machine.screen.vmLabs")}
      tone={tone}
      status={t(VM_STATUS[state])}
      detail={<VmDetail vm={vm} report={row.report} last={row.last} now={row.now} />}
      hint={provider ? freeHint : undefined}
      actions={<VmActions vm={vm} row={row} onUseServer={onUseServer} />}
      testing={row.testing}
      onTestDone={row.onTestDone}
    />
  );
}

/** The hypervisor and its last test once it runs, else why it doesn't. */
function VmDetail({ vm, report, last, now }: { vm: Vm; report: SystemReport; last: LastTest | null; now: number }) {
  const t = useT();
  const { state, provider } = vm;
  if (state === "notApplicable") return t("machine.screen.vm.notApplicable");
  if ((state === "ready" || state === "testFailed") && provider)
    return (
      <>
        {providerLabel(provider, report.os)}
        {vm.automatic ? t("machine.screen.vm.automatic") : ""} · <TestLine test={last} now={now} />
      </>
    );
  if (state === "blocked") return vm.blockedReason;
  return state === "vagrantMissing" ? t("machine.screen.vm.vagrantMissing") : t("machine.screen.vm.needHypervisor");
}

/** A server where VM labs can't run here, the test (and a server when memory is short) where
 *  they can, else the fix. */
function VmActions({ vm, row, onUseServer }: { vm: Vm; row: Omit<RowProps, "report">; onUseServer: () => void }) {
  const t = useT();
  const { state } = vm;
  const server = (variant: "outline" | "ghost") => (
    <Button variant={variant} size="xs" onClick={onUseServer}>
      {t("machine.screen.useServer")}
    </Button>
  );
  if (state === "notApplicable" || state === "blocked") return server("outline");
  if (state === "ready" || state === "testFailed")
    return (
      <>
        {lowFor("vm", row.memFree) && server("ghost")}
        <TestButton testing={row.testing} onClick={row.onToggleTest} />
      </>
    );
  return <FixButton step={state === "vagrantMissing" ? "vagrant" : "vm"} onFix={row.onFix} />;
}
