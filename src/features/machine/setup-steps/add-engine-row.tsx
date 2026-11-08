"use client";

import { ChoiceAction } from "@/components/ui/choice-card";
import { StepCircle } from "@/features/machine/step-row";
import { EngineLogo } from "@/features/machine/engine-logo";
import { EngineAction, EngineHints } from "@/features/machine/setup-steps/engine-action";
import { ProviderReady } from "@/features/machine/setup-steps/add-ready";
import { engineName, enginesFor, recommendedEngine, type Engine } from "@/features/machine/setup-steps/engines";
import { Log } from "@/features/machine/setup-steps/parts";
import { isDockerReady } from "@/features/machine/setup-steps/steps";
import { useEngineActions } from "@/features/machine/setup-steps/use-engine-actions";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import type { SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** One container engine this OS can run but doesn't have: its install (or where to get it, then
 *  a re-check), its log, and once the machine check finds it, its own test and "Use by default". */
export function AddEngineRow({ engine, report, setup }: { engine: Engine; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const found = report.dockerEngines.find((e) => e.engine === engine.id);
  const actions = useEngineActions(setup.onRefresh);
  return (
    <div className="space-y-3 border-t border-border px-4 py-3 first:border-t-0">
      <div className="flex items-center gap-3 text-left">
        <EngineLogo engine={engine} />
        <div className="min-w-0 flex-1">
          <p className="text-[0.8125rem] font-medium text-foreground">{engine.name}</p>
          <p className="text-[0.75rem] text-muted-foreground">{t(engine.note)}</p>
        </div>
        {found && <StepCircle state="done" />}
      </div>
      {found ? (
        <Installed engineId={engine.id} context={found.context} current={found.current} running={found.running} setup={setup} />
      ) : (
        <>
          <ChoiceAction>
            <EngineAction
              report={report}
              setup={setup}
              choice={engine}
              engines={enginesFor(report.os)}
              recommended={recommendedEngine(report.os)}
              actions={actions}
              ready={isDockerReady(report)}
            />
          </ChoiceAction>
          {setup.installerOpened && <EngineHints report={report} choice={engine} installerOpened />}
        </>
      )}
      <Log setup={setup} of={["docker"]} />
    </div>
  );
}

/** The engine is here: test it on its own context, and once the test passes, make it the one
 *  labs use (unless it is already). */
function Installed({
  engineId,
  context,
  current,
  running,
  setup,
}: {
  engineId: Engine["id"];
  context: string;
  current: boolean;
  running: boolean;
  setup: MachineSetupState;
}) {
  const t = useT();
  const actions = useEngineActions(setup.onRefresh);
  return (
    <ProviderReady
      note={running ? t("machine.add.installed") : t("machine.engine.notRunning", { engine: engineName(engineId) })}
      kind="docker"
      target={{ provider: engineId, context, primary: current }}
      canTest={running}
      isDefault={current}
      busy={actions.switching}
      onUseDefault={() => {
        actions.switchTo(engineId);
      }}
    />
  );
}
