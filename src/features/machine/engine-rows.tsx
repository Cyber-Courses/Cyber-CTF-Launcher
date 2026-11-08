"use client";

import { Container } from "lucide-react";
import { TypeIcon } from "@/components/ui/type-icon";
import { EngineMark, engineName } from "@/features/machine/setup-steps";
import { ENGINES, enginesFor } from "@/features/machine/setup-steps/engines";
import { ProviderRow } from "@/features/machine/machine-parts";
import { dockerState, type DockerState } from "@/features/machine/machine-status";
import { FixButton, TESTED_TONE, TestButton, TestLine, useFreeHint, type RowProps } from "@/features/machine/lab-row-parts";
import type { DockerEngine, EngineStatus, SystemReport } from "@/lib/tauri";
import { useT, type T } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

// The Machine page's container engines, one row per engine found on this machine (each Docker
// CLI context asked in turn by the machine check): the current one with its state and the
// container labs' last test, the others running or stopped, with the way to fix, switch or test
// them (the guided setup's engine step, or the inline self-test). Only what is installed: no row
// for an engine this machine lacks, "Add a provider" covers those.

/** The engine's brand mark, or a plain tile for one without (Podman). */
function EngineIcon({ id }: { id: DockerEngine }) {
  if (ENGINES.some((e) => e.id === id)) return <EngineMark id={id} />;
  return (
    <TypeIcon>
      <Container className="size-4" />
    </TypeIcon>
  );
}

export function EngineRows(row: RowProps) {
  const { report, last } = row;
  const state = dockerState(report, last);
  return (
    <>
      {report.dockerEngines.map((e) =>
        e.current ? (
          <CurrentEngineRow key={e.engine} engine={e} state={state} {...row} />
        ) : (
          <OtherEngineRow key={e.engine} engine={e} report={report} onFix={row.onFix} />
        ),
      )}
    </>
  );
}

/** Why the current engine doesn't run labs: this user may not use it, or it is stopped. */
function notRunningDetail(t: T, report: SystemReport, state: DockerState) {
  if (state !== "denied") return t("machine.screen.docker.stopped");
  return report.dockerDeniedHint ? describeError(report.dockerDeniedHint, t) : t("machine.screen.docker.denied");
}

const STATUS = { ready: "ready", testFailed: "testFailed", denied: "noPermission", stopped: "engineStopped", missing: "needsSetup" } as const;

/** The current engine (the CLI's context, which labs run on): ready (test it), denied, or
 *  stopped (fix it). */
function CurrentEngineRow({ engine, state, ...row }: RowProps & { engine: EngineStatus; state: DockerState }) {
  const t = useT();
  const { report, last, now } = row;
  const ready = state === "ready" || state === "testFailed";
  const freeHint = useFreeHint("docker", row.memFree);
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={engine.engine} />}
      title={engineName(engine.engine)}
      badge={t("machine.screen.badge.current")}
      tone={ready ? TESTED_TONE[state] : "warn"}
      status={t(`machine.screen.status.${STATUS[state]}`)}
      detail={ready ? <TestLine test={last} now={now} /> : notRunningDetail(t, report, state)}
      hint={ready ? freeHint : undefined}
      actions={ready ? <TestButton testing={row.testing} onClick={row.onToggleTest} /> : state !== "denied" && <FixButton step="docker" onFix={row.onFix} />}
      testing={row.testing}
      onTestDone={row.onTestDone}
    />
  );
}

/** Another engine found here: running beside the current one (the setup's engine step switches
 *  to it), or stopped (it starts it). */
function OtherEngineRow({ engine, report, onFix }: { engine: EngineStatus; report: SystemReport; onFix: (step: string) => void }) {
  const t = useT();
  const note = enginesFor(report.os).find((e) => e.id === engine.engine)?.note;
  const idle = engine.running && report.dockerEngine ? t("machine.screen.engine.runningIdle", { engine: engineName(report.dockerEngine) }) : null;
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={engine.engine} />}
      title={engineName(engine.engine)}
      tone={engine.running ? "ok" : "muted"}
      status={engine.running ? t("machine.screen.status.running") : t("machine.screen.status.engineStopped")}
      detail={idle ?? (note ? t(note) : engine.context)}
      actions={<FixButton step="docker" onFix={onFix} />}
      testing={false}
      onTestDone={() => {}}
    />
  );
}
