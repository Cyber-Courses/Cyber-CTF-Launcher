"use client";

import { StepCircle } from "@/features/machine/step-row";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { INSTALLABLE, providerLabel, vagrantDrives } from "@/features/machine/hypervisors";
import { ProviderReady } from "@/features/machine/setup-steps/add-ready";
import { Log } from "@/features/machine/setup-steps/parts";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { GetHypervisor, VagrantRequirements } from "@/features/machine/setup-steps/vm-steps";
import { UTILITY } from "@/features/machine/setup-steps/utility-requirement";
import { useVmProvider } from "@/lib/use-vm-provider";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** One hypervisor this OS can run but doesn't have: its install (or where to get it, then a
 *  re-check), then what Vagrant needs to drive it (with VMware, HashiCorp's utility too), and once all is in, its own test and "Use by
 *  default" (the Settings choice). `p` is the hypervisor as the live machine check reports it. */
export function AddHypervisorRow({ p, report, setup }: { p: ProviderStatus; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const installed = p.hypervisor === true;
  return (
    <div className="space-y-3 border-t border-border px-4 py-3 first:border-t-0">
      <div className="flex items-center gap-3 text-left">
        <HypervisorLogo provider={p.provider} />
        <div className="min-w-0 flex-1">
          <p className="text-[0.8125rem] font-medium text-foreground">{providerLabel(p, report.os)}</p>
          <p className="text-[0.75rem] text-muted-foreground">{INSTALLABLE[p.provider] ? t("machine.vm.canInstall") : t("machine.vm.vendorSite")}</p>
        </div>
        {installed && <StepCircle state="done" />}
      </div>
      {!installed && <GetHypervisor choice={p} report={report} setup={setup} />}
      {installed && <VagrantRequirements choice={p} report={report} setup={setup} />}
      {installed && vagrantDrives(p) && <Ready p={p} setup={setup} />}
      <Log setup={setup} of={[p.provider, "vagrant", p.plugin, UTILITY]} />
    </div>
  );
}

/** The hypervisor, Vagrant and its plugin are in: test VM labs on it, and once the test passes,
 *  make it the one labs run on (unless it is already). */
function Ready({ p, setup }: { p: ProviderStatus; setup: MachineSetupState }) {
  const t = useT();
  const preferred = useVmProvider();
  const isDefault = preferred === p.provider;
  return (
    <ProviderReady
      note={t("machine.add.installed")}
      kind="vm"
      target={{ provider: p.provider, primary: isDefault }}
      isDefault={isDefault}
      onUseDefault={() => {
        setup.setHypervisor(p.provider);
      }}
    />
  );
}
