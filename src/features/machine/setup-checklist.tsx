"use client";

import { Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { StepRow } from "@/features/machine/step-row";
import { PATHS, machineSteps, pathSteps, stepLine, stepMeta, type LastTests, type MachineStep, type SetupPath } from "@/features/machine/setup-steps";
import { useVmProvider } from "@/lib/use-vm-provider";
import type { SystemReport } from "@/lib/tauri";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { useT, type MessageKey } from "@/lib/i18n";

const PATH_NAMES: Record<SetupPath, MessageKey> = { docker: "machine.setup.paths.docker", vm: "machine.setup.paths.vm", cloud: "machine.setup.paths.cloud" };

/** The checklist's rows: the steps every path shares first, then each path's own under its name. */
function sections(report: SystemReport): { path: SetupPath | null; steps: MachineStep[] }[] {
  const shared = machineSteps(report, []);
  return [{ path: null, steps: shared }, ...PATHS.map((path) => ({ path, steps: pathSteps(path, report).filter((s) => !shared.includes(s)) }))];
}

/** The Machine page's Setup view: each guided-setup step as a checklist row, with a way into
 *  the setup window at that step. */
export function SetupChecklist({ report, last, onFix }: { report: SystemReport; last: LastTests; onFix: (step: string) => void }) {
  const t = useT();
  const openSetup = useOpenSetup();
  const preferred = useVmProvider();
  const groups = sections(report).map((g) => ({
    ...g,
    steps: g.steps.map((step) => ({ step, title: stepMeta(step, report, t).title, ...stepLine(step, report, last, preferred, t) })),
  }));
  const steps = groups.flatMap((g) => g.steps);
  const done = steps.filter((s) => s.done).length;
  return (
    <Panel>
      <PanelHeader
        title={t("machine.checklist.title")}
        meta={<span className="tabular-nums">{t("machine.checklist.done", { done, total: steps.length })}</span>}
        action={
          // The page header carries the primary "Set up this machine" when something is missing.
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              openSetup();
            }}
          >
            <Wrench /> {t("machine.checklist.open")}
          </Button>
        }
      />
      <div className="py-1.5">
        {groups.map((g) => (
          <div key={g.path ?? "shared"}>
            {g.path && <p className="px-4 pt-2.5 pb-0.5 text-[0.6875rem] font-medium tracking-wide text-faint uppercase">{t(PATH_NAMES[g.path])}</p>}
            {g.steps.map((s) => (
              <StepRow
                key={s.step}
                state={s.done ? "done" : "pending"}
                label={s.title}
                meta={s.meta}
                action={
                  !s.done && (
                    <Button
                      variant={s.optional ? "ghost" : "outline"}
                      size="xs"
                      onClick={() => {
                        onFix(s.step);
                      }}
                    >
                      {s.step.endsWith("-test") ? t("machine.checklist.test") : s.optional ? t("machine.checklist.add") : t("machine.checklist.setUp")}
                    </Button>
                  )
                }
              />
            ))}
          </div>
        ))}
      </div>
    </Panel>
  );
}
