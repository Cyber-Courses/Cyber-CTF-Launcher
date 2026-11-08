"use client";

import { type SystemReport } from "@/lib/tauri";
import { EngineLogo } from "@/features/machine/engine-logo";
import { engineBadge, enginesFor, recommendedEngine } from "@/features/machine/setup-steps/engines";
import { EngineAction, EngineHints } from "@/features/machine/setup-steps/engine-action";
import { Log } from "@/features/machine/setup-steps/parts";
import { Choice, ChoiceAction, ChoiceGrid } from "@/components/ui/choice-card";
import { chosenEngine, isDockerReady } from "@/features/machine/setup-steps/steps";
import { useEngineActions } from "@/features/machine/setup-steps/use-engine-actions";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { useT } from "@/lib/i18n";

export function EngineStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const ready = isDockerReady(report);
  const engines = enginesFor(report.os);
  const recommended = recommendedEngine(report.os);
  // One engine is enough; the pick lives in the setup state so the flow can wait for it.
  const choice = chosenEngine(report, setup);
  const actions = useEngineActions(setup.onRefresh);
  const inUse = report.dockerEngine === choice.id;
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {engines.map((e) => (
          <Choice
            key={e.id}
            selected={e.id === choice.id}
            onSelect={() => {
              setup.setEngine(e.id);
            }}
            mark={<EngineLogo engine={e} />}
            title={e.name}
            note={t(e.note)}
            badge={engineBadge(report, e.id, recommended)}
          />
        ))}
      </ChoiceGrid>
      {/* Only when there is something to do: an engine in use already says so on its card. */}
      {!inUse && (
        <ChoiceAction>
          <EngineAction report={report} setup={setup} choice={choice} engines={engines} recommended={recommended} actions={actions} ready={ready} />
        </ChoiceAction>
      )}
      {!ready && <EngineHints report={report} choice={choice} installerOpened={setup.installerOpened} />}
      <Log setup={setup} of={["docker"]} />
      <EngineTrademarks />
    </div>
  );
}

function EngineTrademarks() {
  const t = useT();
  return <p className="pt-1 text-left text-[0.6875rem] leading-relaxed text-faint">{t("machine.engine.trademarks")}</p>;
}
