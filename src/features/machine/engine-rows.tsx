"use client";

import { Container } from "lucide-react";
import { TypeIcon } from "@/components/ui/type-icon";
import { EngineMark, engineName } from "@/features/machine/setup-steps";
import { ENGINES, enginesFor } from "@/features/machine/setup-steps/engines";
import { ProviderRow, type RowStatus } from "@/features/machine/provider-row";
import { dockerState, type DockerState } from "@/features/machine/machine-status";
import { FixButton, RowTestButton, TestLine, testKey, testedStatus, useFreeHint, rowTest, type RowProps } from "@/features/machine/lab-row-parts";
import { getLastTest, type LastTest } from "@/lib/settings";
import { dockerUseEngine, type DockerEngine, type EngineStatus, type SystemReport } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT, type T } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

// The Machine page's container engines, one row per engine found on this machine (each Docker
// CLI context asked in turn by the machine check): the current one with its state, the others
// running or stopped. Every running engine has its own test, run on its own context, and shows
// its own last result; a running engine can be made the one labs use ("Use by default"). Only
// what is installed: no row for an engine this machine lacks, "Add a provider" covers those.

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
        e.current ? <CurrentEngineRow key={e.engine} engine={e} state={state} {...row} /> : <OtherEngineRow key={e.engine} engine={e} {...row} />,
      )}
    </>
  );
}

/** Why the current engine doesn't run labs: this user may not use it, or it is stopped. */
function notRunningDetail(t: T, report: SystemReport, state: DockerState) {
  if (state !== "denied") return t("machine.screen.docker.stopped");
  return report.dockerDeniedHint ? describeError(report.dockerDeniedHint, t) : t("machine.screen.docker.denied");
}

/** The current engine's row status, by where container labs stand (its own last test when it runs). */
const currentStatus = (state: DockerState, last: LastTest | null): RowStatus =>
  state === "denied" ? "noPermission" : state === "stopped" ? "stopped" : state === "missing" ? "needsSetup" : testedStatus(last);

/** The current engine (the CLI's context, which labs run on: the default): ready (test it),
 *  denied, or stopped (fix it). Its last test, else the container labs' own. */
function CurrentEngineRow({ engine, state, ...row }: RowProps & { engine: EngineStatus; state: DockerState }) {
  const t = useT();
  const { report, now } = row;
  const ready = state === "ready" || state === "testFailed";
  const freeHint = useFreeHint("docker", row.memFree);
  const key = testKey("docker", engine.engine);
  const last = getLastTest("docker", engine.engine) ?? row.last;
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={engine.engine} />}
      title={engineName(engine.engine)}
      status={currentStatus(state, last)}
      isDefault
      detail={ready ? <TestLine test={last} now={now} /> : notRunningDetail(t, report, state)}
      hint={ready ? freeHint : undefined}
      actions={ready ? <RowTestButton row={row} id={key} /> : state !== "denied" && <FixButton step="docker" onFix={row.onFix} />}
      {...rowTest(row, key)}
      target={{ provider: engine.engine, context: engine.context, primary: true }}
    />
  );
}

/** Another engine found here: running (its own test, on its context; "Use by default" points
 *  Docker at it), or stopped (the setup's engine step starts it). */
function OtherEngineRow({ engine, ...row }: RowProps & { engine: EngineStatus }) {
  const t = useT();
  const { report, now } = row;
  const note = enginesFor(report).find((e) => e.id === engine.engine)?.note;
  const key = testKey("docker", engine.engine);
  if (!engine.running)
    return (
      <ProviderRow
        kind="docker"
        icon={<EngineIcon id={engine.engine} />}
        title={engineName(engine.engine)}
        status="stopped"
        detail={note ? t(note) : engine.context}
        actions={<FixButton step="docker" onFix={row.onFix} />}
        testing={false}
        onTestDone={() => {}}
      />
    );
  const last = getLastTest("docker", engine.engine);
  return (
    <ProviderRow
      kind="docker"
      icon={<EngineIcon id={engine.engine} />}
      title={engineName(engine.engine)}
      status={testedStatus(last)}
      onUseDefault={() => {
        dockerUseEngine(engine.engine)
          .then(row.onRefresh)
          .catch(tell(t("machine.errors.switchEngine")));
      }}
      detail={<TestLine test={last} now={now} />}
      actions={<RowTestButton row={row} id={key} />}
      {...rowTest(row, key)}
      target={{ provider: engine.engine, context: engine.context, primary: false }}
    />
  );
}
