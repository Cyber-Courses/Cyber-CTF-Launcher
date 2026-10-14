"use client";

import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { type SystemReport } from "@/lib/tauri";
import { cantRun, DOWNLOAD, INSTALLABLE, providerLabel, usableHypervisors } from "@/features/machine/hypervisors";
import { Log, Requirement } from "@/features/machine/setup-steps/parts";
import { Choice, ChoiceAction, ChoiceGrid } from "@/components/ui/choice-card";
import { chosenHypervisor } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

export function VmStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const hypervisors = usableHypervisors(report);
  const { installing, install, onRefresh } = setup;
  // One hypervisor is enough; the pick lives in the setup state so the flow can wait for it.
  const picked = chosenHypervisor(report, setup);
  if (hypervisors.length === 0) {
    return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.noHypervisor")}</p>;
  }
  const choice = picked ?? hypervisors[0];
  const label = providerLabel(choice, report.os);
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {hypervisors.map((p) => (
          <Choice
            key={p.provider}
            selected={p.provider === choice.provider}
            onSelect={() => setup.setHypervisor(p.provider)}
            mark={<HypervisorLogo provider={p.provider} />}
            title={providerLabel(p, report.os)}
            note={
              p.hypervisor === true
                ? cantRun(p, report)
                  ? t("machine.vm.cantRunHere")
                  : t("machine.vm.installedHere")
                : INSTALLABLE[p.provider]
                  ? t("machine.vm.canInstall")
                  : t("machine.vm.vendorSite")
            }
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
      {choice.hypervisor !== true && (
        <ChoiceAction>
          {INSTALLABLE[choice.provider] ? (
            <>
              <span className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.canInstallNamed", { name: label })}</span>
              <Button
                variant="primary"
                size="sm"
                onClick={() => install(choice.provider, INSTALLABLE[choice.provider]!, t("machine.vm.installLog", { name: label }))}
                disabled={installing !== null}
              >
                {installing === choice.provider ? (
                  <>
                    <Spinner className="size-3.5" /> {t("machine.vm.installing")}
                  </>
                ) : (
                  t("machine.vm.install", { name: label })
                )}
              </Button>
            </>
          ) : (
            <>
              <span className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.installThenRecheck", { name: label })}</span>
              <span className="flex shrink-0 gap-2">
                {DOWNLOAD[choice.provider] && (
                  <Button variant="outline" size="sm" onClick={() => openExternal(DOWNLOAD[choice.provider]!)}>
                    <ExternalLink className="size-3.5" /> {t("machine.vm.get", { name: label })}
                  </Button>
                )}
                <Button variant="outline" size="sm" onClick={() => onRefresh()}>
                  <RefreshCw className="size-3.5" /> {t("machine.vm.recheck")}
                </Button>
              </span>
            </>
          )}
        </ChoiceAction>
      )}
      <Log setup={setup} of={[choice.provider]} />
      <p className="pt-1 text-left text-[0.6875rem] leading-relaxed text-faint">{t("machine.vm.trademarks")}</p>
    </div>
  );
}

export function VagrantStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const choice = chosenHypervisor(report, setup);
  const { installing, install } = setup;
  if (!choice) {
    return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.vagrantNothing")}</p>;
  }
  const label = providerLabel(choice, report.os);
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
            <Button variant="primary" size="xs" onClick={() => install("vagrant", "vagrant", t("machine.vm.installVagrantLog"))} disabled={installing !== null}>
              {installing === "vagrant" ? (
                <>
                  <Spinner className="size-3" /> {t("machine.vm.installing")}
                </>
              ) : (
                t("machine.vm.installVagrant")
              )}
            </Button>
          }
        />
        {choice.plugin && (
          <Requirement
            ok={choice.pluginInstalled}
            title={t("machine.vm.pluginTitle", { name: label })}
            detail={choice.pluginInstalled || report.vagrant.installed ? choice.plugin : t("machine.vm.pluginLater", { plugin: choice.plugin })}
            action={
              <Button
                variant={report.vagrant.installed ? "primary" : "outline"}
                size="xs"
                onClick={() => setup.installPlugin(choice.plugin!)}
                disabled={installing !== null || !report.vagrant.installed}
              >
                {installing === choice.plugin ? (
                  <>
                    <Spinner className="size-3" /> {t("machine.vm.installing")}
                  </>
                ) : (
                  t("machine.vm.installPlugin")
                )}
              </Button>
            }
          />
        )}
        {choice.provider === "vmware_desktop" && (
          <Requirement
            ok={false}
            optional
            title="Vagrant VMware Utility"
            detail={t("machine.vm.utilityDetail")}
            action={
              <Button variant="outline" size="xs" onClick={() => openExternal("https://developer.hashicorp.com/vagrant/install/vmware")}>
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

/** One thing the step needs: done (with a check), or its install action. `optional` rows
 *  can't be detected, so they always show their action and never block the flow. */
