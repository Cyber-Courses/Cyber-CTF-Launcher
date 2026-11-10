"use client";

import { Button } from "@/components/ui/button";
import { type ProviderStatus, type SystemReport } from "@/lib/tauri";
import { UTILITY, UtilityRequirement } from "@/features/machine/setup-steps/utility-requirement";
import { providerLabel } from "@/features/machine/hypervisors";
import { BusyLabel, Log, OwnInstall, Requirement } from "@/features/machine/setup-steps/parts";
import { actionFor, vagrantActions } from "@/features/machine/setup-steps/plan";
import { chosenHypervisor } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { useT } from "@/lib/i18n";

// The Vagrant step: what Vagrant needs to drive the hypervisor picked on the VM step, as the
// core's plan lists it.

export function VagrantStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const choice = chosenHypervisor(report, setup);
  if (!choice) {
    return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.vagrantNothing")}</p>;
  }
  return (
    <div className="space-y-3">
      {/* For the hypervisor picked on the previous step. */}
      <VagrantRequirements choice={choice} report={report} setup={setup} />
      <Log setup={setup} of={["vagrant", choice.plugin, UTILITY]} />
    </div>
  );
}

/** What Vagrant needs to drive `choice`: Vagrant itself, the hypervisor's plugin, and (VMware)
 *  HashiCorp's utility; each with its install, the first missing one the jewel. The rows are the
 *  backend's `needs` (what its readiness reads), so once each is done the hypervisor is ready. */
export function VagrantRequirements({ choice, report, setup }: { choice: ProviderStatus; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const actions = vagrantActions(report, choice.provider);
  const vagrant = actionFor(actions, "vagrant");
  const utility = actionFor(actions, UTILITY);
  return (
    <>
      <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
        <Requirement
          ok={!vagrant}
          title="Vagrant"
          detail={
            !vagrant
              ? (report.vagrant.version ?? t("machine.body.installed"))
              : vagrant.kind === "manual"
                ? t("machine.body.distribution", { name: "Vagrant" })
                : t("machine.vm.vagrantDetail")
          }
          failed={setup.installs.failure("vagrant")}
          action={
            vagrant?.kind === "manual" ? (
              <OwnInstall action={vagrant} name="Vagrant" />
            ) : (
              // The first thing missing is the jewel: Vagrant, then its plugin (one per view).
              <Button
                variant="primary"
                size="xs"
                onClick={() => {
                  setup.installs.install("vagrant", "vagrant", t("machine.vm.installVagrantLog"));
                }}
                disabled={!setup.installs.canInstall}
              >
                <BusyLabel busy={setup.installs.installing === "vagrant"} label={t("machine.vm.installing")} spinner="size-3">
                  {t("machine.vm.installVagrant")}
                </BusyLabel>
              </Button>
            )
          }
        />
        {choice.plugin && (
          <PluginRequirement plugin={choice.plugin} ok={!actions.some((a) => a.kind === "plugin")} choice={choice} report={report} setup={setup} />
        )}
        {choice.utility && <UtilityRequirement utility={choice.utility} action={utility} first={actions[0] === utility} setup={setup} />}
      </div>
    </>
  );
}

/** The Vagrant plugin that drives the chosen hypervisor: installed once Vagrant is. */
function PluginRequirement({
  plugin,
  ok,
  choice,
  report,
  setup,
}: {
  plugin: string;
  ok: boolean;
  choice: ProviderStatus;
  report: SystemReport;
  setup: MachineSetupState;
}) {
  const t = useT();
  return (
    <Requirement
      ok={ok}
      title={t("machine.vm.pluginTitle", { name: providerLabel(choice, report.os) })}
      detail={ok || report.vagrant.installed ? plugin : t("machine.vm.pluginLater", { plugin })}
      failed={setup.installs.failure(plugin)}
      action={
        <Button
          variant={report.vagrant.installed ? "primary" : "outline"}
          size="xs"
          onClick={() => {
            setup.installs.installPlugin(plugin);
          }}
          disabled={!setup.installs.canInstall || !report.vagrant.installed}
        >
          <BusyLabel busy={setup.installs.installing === plugin} label={t("machine.vm.installing")} spinner="size-3">
            {t("machine.vm.installPlugin")}
          </BusyLabel>
        </Button>
      }
    />
  );
}
