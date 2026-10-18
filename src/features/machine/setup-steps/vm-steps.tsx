"use client";

import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { type ProviderStatus, type SystemReport } from "@/lib/tauri";
import { cantRun, DOWNLOAD, INSTALLABLE, providerLabel, usableHypervisors } from "@/features/machine/hypervisors";
import { BusyLabel, Log, Requirement } from "@/features/machine/setup-steps/parts";
import { Choice, ChoiceAction, ChoiceGrid } from "@/components/ui/choice-card";
import { chosenHypervisor } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { openExternal } from "@/lib/failure";
import { useT, type T } from "@/lib/i18n";

/** A hypervisor's card note: installed (able to run here or not), installable here, or the vendor's. */
function choiceNote(t: T, p: ProviderStatus, report: SystemReport) {
  if (p.hypervisor === true) return cantRun(p, report) ? t("machine.vm.cantRunHere") : t("machine.vm.installedHere");
  return INSTALLABLE[p.provider] ? t("machine.vm.canInstall") : t("machine.vm.vendorSite");
}

export function VmStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const hypervisors = usableHypervisors(report);
  // One hypervisor is enough; the pick lives in the setup state so the flow can wait for it.
  const choice = chosenHypervisor(report, setup);
  if (!choice) return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.noHypervisor")}</p>;
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {hypervisors.map((p) => (
          <Choice
            key={p.provider}
            selected={p.provider === choice.provider}
            onSelect={() => {
              setup.setHypervisor(p.provider);
            }}
            mark={<HypervisorLogo provider={p.provider} />}
            title={providerLabel(p, report.os)}
            note={choiceNote(t, p, report)}
            badge={p.hypervisor === true ? "installed" : undefined}
          />
        ))}
      </ChoiceGrid>
      {/* Installed but unable to run (no /dev/kvm, not in the kvm group): why, as the Machine page says. */}
      {cantRun(choice, report) && choice.reason && (
        <ChoiceAction>
          <span className="text-[0.8125rem] text-muted-foreground">
            {choice.reason.charAt(0).toUpperCase()}
            {choice.reason.slice(1)}.
          </span>
        </ChoiceAction>
      )}
      {/* Only when there is something to do: an installed hypervisor says so on its card. */}
      {choice.hypervisor !== true && <GetHypervisor choice={choice} report={report} setup={setup} />}
      <Log setup={setup} of={[choice.provider]} />
      <p className="pt-1 text-left text-[0.6875rem] leading-relaxed text-faint">{t("machine.vm.trademarks")}</p>
    </div>
  );
}

/** A hypervisor not installed: its one-click install, or a link to its vendor and a re-check. */
function GetHypervisor({ choice, report, setup }: { choice: ProviderStatus; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const label = providerLabel(choice, report.os);
  const dependency = INSTALLABLE[choice.provider];
  const download = DOWNLOAD[choice.provider];
  if (dependency)
    return (
      <ChoiceAction>
        <span className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.canInstallNamed", { name: label })}</span>
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            setup.install(choice.provider, dependency, t("machine.vm.installLog", { name: label }));
          }}
          disabled={setup.installing !== null}
        >
          <BusyLabel busy={setup.installing === choice.provider} label={t("machine.vm.installing")}>
            {t("machine.vm.install", { name: label })}
          </BusyLabel>
        </Button>
      </ChoiceAction>
    );
  return (
    <ChoiceAction>
      <span className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.installThenRecheck", { name: label })}</span>
      <span className="flex shrink-0 gap-2">
        {download && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              openExternal(download);
            }}
          >
            <ExternalLink className="size-3.5" /> {t("machine.vm.get", { name: label })}
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            setup.onRefresh();
          }}
        >
          <RefreshCw className="size-3.5" /> {t("machine.vm.recheck")}
        </Button>
      </span>
    </ChoiceAction>
  );
}

export function VagrantStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const choice = chosenHypervisor(report, setup);
  if (!choice) {
    return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.vagrantNothing")}</p>;
  }
  return (
    <div className="space-y-3">
      {/* For the hypervisor picked on the previous step. */}
      <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
        <Requirement
          ok={report.vagrant.installed}
          title="Vagrant"
          detail={report.vagrant.installed ? (report.vagrant.version ?? t("machine.body.installed")) : t("machine.vm.vagrantDetail")}
          action={
            // The first thing missing is the jewel: Vagrant, then its plugin (one per view).
            <Button
              variant="primary"
              size="xs"
              onClick={() => {
                setup.install("vagrant", "vagrant", t("machine.vm.installVagrantLog"));
              }}
              disabled={setup.installing !== null}
            >
              <BusyLabel busy={setup.installing === "vagrant"} label={t("machine.vm.installing")} spinner="size-3">
                {t("machine.vm.installVagrant")}
              </BusyLabel>
            </Button>
          }
        />
        {choice.plugin && <PluginRequirement plugin={choice.plugin} choice={choice} report={report} setup={setup} />}
        {choice.provider === "vmware_desktop" && (
          <Requirement
            ok={false}
            optional
            title="Vagrant VMware Utility"
            detail={t("machine.vm.utilityDetail")}
            action={
              <Button
                variant="outline"
                size="xs"
                onClick={() => {
                  openExternal("https://developer.hashicorp.com/vagrant/install/vmware");
                }}
              >
                <ExternalLink /> {t("machine.vm.getUtility")}
              </Button>
            }
          />
        )}
      </div>
      <Log setup={setup} of={["vagrant", choice.plugin]} />
    </div>
  );
}

/** The Vagrant plugin that drives the chosen hypervisor: installed once Vagrant is. */
function PluginRequirement({ plugin, choice, report, setup }: { plugin: string; choice: ProviderStatus; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  return (
    <Requirement
      ok={choice.pluginInstalled}
      title={t("machine.vm.pluginTitle", { name: providerLabel(choice, report.os) })}
      detail={choice.pluginInstalled || report.vagrant.installed ? plugin : t("machine.vm.pluginLater", { plugin })}
      action={
        <Button
          variant={report.vagrant.installed ? "primary" : "outline"}
          size="xs"
          onClick={() => {
            setup.installPlugin(plugin);
          }}
          disabled={setup.installing !== null || !report.vagrant.installed}
        >
          <BusyLabel busy={setup.installing === plugin} label={t("machine.vm.installing")} spinner="size-3">
            {t("machine.vm.installPlugin")}
          </BusyLabel>
        </Button>
      }
    />
  );
}
