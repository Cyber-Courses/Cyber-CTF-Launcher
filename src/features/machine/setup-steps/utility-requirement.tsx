"use client";

import { Button } from "@/components/ui/button";
import { BusyLabel, Requirement } from "@/features/machine/setup-steps/parts";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import type { Dependency, ProviderStatus, UtilityState } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The utility's install, and its id in the install log. */
export const UTILITY = "vmware-utility" satisfies Dependency;

/** HashiCorp's Vagrant VMware Utility, the service VMware's plugin drives VMware through: done
 *  only once the machine check finds it running; installed from HashiCorp's signed package (the
 *  OS asks for an administrator's password). Installed but stopped: installing again restarts it. */
export function UtilityRequirement({ utility, choice, setup }: { utility: UtilityState; choice: ProviderStatus; setup: MachineSetupState }) {
  const t = useT();
  // The jewel once nothing before it is missing: Vagrant and the plugin go first.
  const next = choice.needs[0] === "vmwareUtility";
  return (
    <Requirement
      ok={utility === "running"}
      title={t("machine.vm.utilityTitle")}
      detail={utility === "stopped" ? t("machine.vm.utilityStopped") : t("machine.vm.utilityDetail")}
      failed={setup.failure(UTILITY)}
      action={
        <Button
          variant={next ? "primary" : "outline"}
          size="xs"
          onClick={() => {
            setup.install(UTILITY, UTILITY, t("machine.vm.utilityLog"));
          }}
          disabled={!setup.canInstall}
        >
          <BusyLabel busy={setup.installing === UTILITY} label={t("machine.vm.installing")} spinner="size-3">
            {utility === "stopped" ? t("machine.vm.utilityRestart") : t("machine.vm.utilityInstall")}
          </BusyLabel>
        </Button>
      }
    />
  );
}
