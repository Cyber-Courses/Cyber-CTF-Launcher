"use client";

import { Container } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TypeIcon } from "@/components/ui/type-icon";
import type { Tone } from "@/components/ui/status-pill";
import { EngineMark, engineName } from "@/features/machine/setup-steps";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { providerLabel } from "@/features/machine/hypervisors";
import { LabTypeRow, type LabKind } from "@/features/machine/machine-parts";
import { WANT_FREE, dockerState, type vmSetup } from "@/features/machine/machine-status";
import { formatAgo, formatBytes } from "@/lib/format";
import type { SystemReport } from "@/lib/tauri";
import type { LastTest } from "@/lib/settings";
import { useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

type RowProps = {
  report: SystemReport;
  last: LastTest | null;
  now: number;
  /** Free memory now (null until the first metrics read). */
  memFree: number | null;
  testing: boolean;
  onToggleTest: () => void;
  onTestDone: () => void;
  /** Opens the setup window at a step. */
  onFix: (step: string) => void;
};

const TONE = { ready: "ok", testFailed: "fail" } as const;

/** Container labs on this machine: the engine, its last test, and the fix or the test. */
export function DockerLabRow({ report, last, now, memFree, testing, onToggleTest, onTestDone, onFix }: RowProps) {
  const t = useT();
  const parts = useRowParts(now, memFree);
  const state = dockerState(report, last);
  const ready = state === "ready" || state === "testFailed";
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
      tone={ready ? TONE[state] : "warn"}
      status={
        state === "ready"
          ? t("machine.screen.status.ready")
          : state === "testFailed"
            ? t("machine.screen.status.testFailed")
            : state === "denied"
              ? t("machine.screen.status.noPermission")
              : state === "stopped"
                ? t("machine.screen.status.engineStopped")
                : t("machine.screen.status.needsSetup")
      }
      detail={
        ready ? (
          <>
            {report.dockerEngine ? engineName(report.dockerEngine) : "Docker"} · {parts.testLine(last)}
          </>
        ) : state === "denied" ? (
          report.dockerDeniedHint ? (
            describeError(report.dockerDeniedHint, t)
          ) : (
            t("machine.screen.docker.denied")
          )
        ) : state === "stopped" ? (
          t("machine.screen.docker.stopped")
        ) : // The engines this OS has (OrbStack is macOS-only; Docker Engine is Linux's own).
        report.os === "linux" ? (
          t("machine.screen.docker.noEngineLinux")
        ) : report.os === "macos" ? (
          t("machine.screen.docker.noEngineMacos")
        ) : (
          t("machine.screen.docker.noEngineWindows")
        )
      }
      hint={ready ? parts.freeHint("docker") : undefined}
      actions={
        ready ? (
          <TestButton testing={testing} onClick={onToggleTest} />
        ) : state === "denied" ? null : (
          <Button variant="outline" size="xs" onClick={() => onFix("docker")}>
            {t("machine.screen.fix")}
          </Button>
        )
      }
      testing={testing}
      onTestDone={onTestDone}
    />
  );
}

/** VM labs on this machine: the hypervisor, its last test, and the fix, the test or a server. */
export function VmLabRow({
  report,
  vm,
  last,
  now,
  memFree,
  testing,
  onToggleTest,
  onTestDone,
  onFix,
  onUseServer,
}: RowProps & { vm: ReturnType<typeof vmSetup>; onUseServer: () => void }) {
  const t = useT();
  const parts = useRowParts(now, memFree);
  const { state, provider } = vm;
  const ready = state === "ready" || state === "testFailed";
  const tone: Tone = state === "notApplicable" ? "muted" : ready ? TONE[state] : "warn";
  const serverButton = (variant: "outline" | "ghost") => (
    <Button variant={variant} size="xs" onClick={onUseServer}>
      {t("machine.screen.useServer")}
    </Button>
  );
  return (
    <LabTypeRow
      kind="vm"
      icon={<HypervisorLogo provider={provider?.provider} />}
      title={t("machine.screen.vmLabs")}
      tone={tone}
      status={
        state === "notApplicable"
          ? t("machine.screen.status.notOnThisMachine")
          : state === "ready"
            ? t("machine.screen.status.ready")
            : state === "testFailed"
              ? t("machine.screen.status.testFailed")
              : t("machine.screen.status.needsSetup")
      }
      detail={
        state === "notApplicable" ? (
          t("machine.screen.vm.notApplicable")
        ) : ready && provider ? (
          <>
            {providerLabel(provider, report.os)}
            {vm.automatic ? t("machine.screen.vm.automatic") : ""} · {parts.testLine(last)}
          </>
        ) : state === "blocked" ? (
          vm.blockedReason
        ) : state === "vagrantMissing" ? (
          t("machine.screen.vm.vagrantMissing")
        ) : (
          t("machine.screen.vm.needHypervisor")
        )
      }
      hint={provider ? parts.freeHint("vm") : undefined}
      actions={
        state === "notApplicable" || state === "blocked" ? (
          serverButton("outline")
        ) : ready ? (
          <>
            {parts.lowFor("vm") && serverButton("ghost")}
            <TestButton testing={testing} onClick={onToggleTest} />
          </>
        ) : (
          <Button variant="outline" size="xs" onClick={() => onFix(state === "vagrantMissing" ? "vagrant" : "vm")}>
            {t("machine.screen.fix")}
          </Button>
        )
      }
      testing={testing}
      onTestDone={onTestDone}
    />
  );
}

/** Opens or hides a lab type's self-test inline. */
function TestButton({ testing, onClick }: { testing: boolean; onClick: () => void }) {
  const t = useT();
  return (
    <Button variant="outline" size="xs" onClick={onClick}>
      {testing ? t("machine.screen.hideTest") : t("machine.screen.test")}
    </Button>
  );
}

/** The lines both rows share: the last test, and a warning when memory is short for the type. */
function useRowParts(now: number, memFree: number | null) {
  const t = useT();
  const lowFor = (kind: LabKind) => memFree !== null && memFree < WANT_FREE[kind];
  return {
    lowFor,
    freeHint: (kind: LabKind) =>
      lowFor(kind)
        ? t(kind === "vm" ? "machine.screen.freeHintVm" : "machine.screen.freeHintDocker", {
            want: formatBytes(WANT_FREE[kind]),
            free: formatBytes(Math.max(0, memFree!)),
          })
        : undefined,
    testLine: (test: LastTest | null) =>
      !test ? (
        <span>{t("machine.screen.notTestedYet")}</span>
      ) : test.result === "ok" ? (
        <span>{t("machine.screen.tested", { ago: formatAgo(test.at, now) })}</span>
      ) : (
        <span className="text-destructive">{t("machine.screen.lastTestFailed", { ago: formatAgo(test.at, now) })}</span>
      ),
  };
}
