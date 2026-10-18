"use client";

import { Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { StepRow } from "@/features/machine/step-row";
import { machineSteps, stepMeta, type MachineStep } from "@/features/machine/setup-steps";
import { hasHypervisor, isDockerReady, wslState } from "@/features/machine/setup-steps/steps";
import { engineName } from "@/features/machine/setup-steps/engines";
import { cantRun, usableHypervisors, providerLabel } from "@/features/machine/hypervisors";
import { ATTACK_PRESETS, getAttackImage, type LastTest } from "@/lib/settings";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { useT, type T } from "@/lib/i18n";

type Line = { done: boolean; optional?: boolean; meta: string };

/** The last self-test per lab type. */
type LastTests = Record<"docker" | "vm", LastTest | null>;

/** What every step's line reads: the report, the last tests, and the hypervisor labs would use. */
type Facts = {
  report: SystemReport;
  last: LastTests;
  t: T;
  /** The installed hypervisor that can run labs, else the first installed one. */
  hv: ProviderStatus | undefined;
  /** It is installed yet can't run here: not done, as the Health tab says. */
  blocked: boolean;
};

const tested = (t: T, r: LastTest | null) => {
  if (!r) return t("machine.checklist.notTested");
  return r.result === "ok" ? t("machine.checklist.passed") : t("machine.checklist.failed");
};

function wslLine({ report, t }: Facts): Line {
  const wsl = wslState(report);
  if (wsl === "ready") return { done: true, meta: "WSL 2" };
  if (wsl === "needsReboot") return { done: false, meta: t("machine.checklist.restartNeeded") };
  if (wsl === "missing") return { done: false, meta: t("machine.checklist.notInstalled") };
  return { done: false, meta: t("machine.checklist.notChecked") };
}

function dockerLine({ report, t }: Facts): Line {
  if (isDockerReady(report)) return { done: true, meta: report.dockerEngine ? engineName(report.dockerEngine) : t("machine.checklist.running") };
  return { done: false, meta: report.docker.installed ? t("machine.checklist.stopped") : t("machine.checklist.notInstalled") };
}

function vagrantLine({ report, t, hv }: Facts): Line {
  const optional = !hasHypervisor(report);
  const missingPlugin = hv?.plugin && !hv.pluginInstalled ? hv.plugin : null;
  if (!report.vagrant.installed) return { done: false, optional, meta: t("machine.checklist.notInstalled") };
  if (missingPlugin) return { done: false, optional, meta: t("machine.checklist.pluginMissing", { plugin: missingPlugin }) };
  return { done: true, optional, meta: t("machine.checklist.installed") };
}

/** Where this machine stands on each guided-setup step, read from the system report and the
 *  last self-tests. The steps themselves run in the setup window. */
const LINES: Record<MachineStep, (f: Facts) => Line> = {
  pkgmgr: ({ report, t }) => ({
    done: report.pkgManager.installed,
    meta: report.pkgManager.installed ? report.pkgManager.name : t("machine.checklist.notInstalled"),
  }),
  virtualization: wslLine,
  docker: dockerLine,
  "docker-test": ({ last, t }) => ({ done: last.docker?.result === "ok", meta: tested(t, last.docker) }),
  attack: () => {
    const image = getAttackImage();
    return { done: true, meta: ATTACK_PRESETS.find((p) => p.image === image)?.label ?? image };
  },
  vm: ({ report, t, hv, blocked }) => ({
    done: hasHypervisor(report) && !blocked,
    optional: true,
    meta: hv ? `${providerLabel(hv, report.os)}${blocked ? t("machine.checklist.cantRunHere") : ""}` : t("machine.checklist.optional"),
  }),
  vagrant: vagrantLine,
  "vm-test": ({ report, last, t }) => ({ done: last.vm?.result === "ok", optional: !hasHypervisor(report), meta: tested(t, last.vm) }),
};

function stepLine(step: MachineStep, report: SystemReport, last: LastTests, t: T): Line {
  const installed = usableHypervisors(report).filter((p) => p.hypervisor === true);
  // One that can run labs first (QEMU without KVM can, when libvirt can't).
  const hv = installed.find((p) => !cantRun(p, report)) ?? installed.at(0);
  return LINES[step]({ report, last, t, hv, blocked: !!hv && cantRun(hv, report) });
}

/** The Machine page's Setup view: each guided-setup step as a checklist row, with a way into
 *  the setup window at that step. */
export function SetupChecklist({ report, last, onFix }: { report: SystemReport; last: LastTests; onFix: (step: string) => void }) {
  const t = useT();
  const openSetup = useOpenSetup();
  const steps = machineSteps(report).map((step) => ({ step, title: stepMeta(step, report, t).title, ...stepLine(step, report, last, t) }));
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
        {steps.map((s) => (
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
    </Panel>
  );
}
