"use client";

import { Container } from "lucide-react";
import { TypeIcon } from "@/components/ui/type-icon";
import { EngineMark, engineName } from "@/features/machine/setup-steps";
import { ENGINES, enginesFor, recommendedEngine } from "@/features/machine/setup-steps/engines";
import { ProviderRow } from "@/features/machine/machine-parts";
import { dockerState, type DockerState } from "@/features/machine/machine-status";
import { FixButton, TESTED_TONE, TestButton, TestLine, useFreeHint, type RowProps } from "@/features/machine/lab-row-parts";
import type { DockerEngine, SystemReport } from "@/lib/tauri";
import { useT, type T } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

// The Machine page's container engines, one row each: the one labs use with its state and the
// container labs' last test, the others running or not set up, with the way to set up, fix or
// test them (the guided setup's engine step, or the inline self-test).

/** The engine labs use: the one Docker's context names, else, with Docker installed but no
 *  engine answering (stopped, or this user not allowed), the one this OS installs by default. */
function currentEngine(report: SystemReport, state: DockerState): DockerEngine | null {
  if (report.dockerEngine) return report.dockerEngine;
  return state === "missing" ? null : recommendedEngine(report.os);
}

/** The engine's brand mark, or a plain tile for one without (Podman). */
function EngineIcon({ id }: { id: DockerEngine }) {
  if (ENGINES.some((e) => e.id === id)) return <EngineMark id={id} />;
  return (
    <TypeIcon>
      <Container className="size-4" />
    </TypeIcon>
  );
}

/** Container engines on this machine: the one labs use with its state and last test, the others
 *  running or not set up. */
export function EngineRows(row: RowProps) {
  const { report, last } = row;
  const state = dockerState(report, last);
  const current = currentEngine(report, state);
  const engines = enginesFor(report.os);
  // An engine in use this OS has no card for (Podman) still gets its row, first.
  const ids: DockerEngine[] = current && !engines.some((e) => e.id === current) ? [current, ...engines.map((e) => e.id)] : engines.map((e) => e.id);
  return (
    <>
      {ids.map((id) =>
        id === current ? <CurrentEngineRow key={id} id={id} state={state} {...row} /> : <OtherEngineRow key={id} id={id} report={report} onFix={row.onFix} />,
      )}
    </>
  );
}

/** Why the engine in use doesn't run labs: this user may not use it, or it is stopped. */
function notRunningDetail(t: T, report: SystemReport, state: DockerState) {
  if (state !== "denied") return t("machine.screen.docker.stopped");
  return report.dockerDeniedHint ? describeError(report.dockerDeniedHint, t) : t("machine.screen.docker.denied");
}

const STATUS = { ready: "ready", testFailed: "testFailed", denied: "noPermission", stopped: "engineStopped", missing: "needsSetup" } as const;

/** The engine labs use: ready (test it), denied, or stopped (fix it). */
function CurrentEngineRow({ id, state, ...row }: RowProps & { id: DockerEngine; state: DockerState }) {
  const t = useT();
  const { report, last, now } = row;
  const ready = state === "ready" || state === "testFailed";
  const freeHint = useFreeHint("docker", row.memFree);
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={id} />}
      title={engineName(id)}
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

/** Another engine this OS has: running beside the one in use (the setup's engine step switches
 *  to it), or not set up (add it there). */
function OtherEngineRow({ id, report, onFix }: { id: DockerEngine; report: SystemReport; onFix: (step: string) => void }) {
  const t = useT();
  const engine = enginesFor(report.os).find((e) => e.id === id);
  const running = report.dockerEnginesRunning.includes(id);
  const idle = running && report.dockerEngine ? t("machine.screen.engine.runningIdle", { engine: engineName(report.dockerEngine) }) : null;
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={id} />}
      title={engineName(id)}
      tone="muted"
      status={running ? t("machine.screen.status.running") : t("machine.screen.status.notSetUp")}
      detail={idle ?? (engine ? t(engine.note) : "")}
      actions={<FixButton step="docker" onFix={onFix} add={!running} />}
      testing={false}
      onTestDone={() => {}}
    />
  );
}
