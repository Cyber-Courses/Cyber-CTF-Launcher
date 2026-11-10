"use client";

import { Button } from "@/components/ui/button";
import { BusyLabel, OwnInstall, Requirement } from "@/features/machine/setup-steps/parts";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import type { Dependency, SetupAction, UtilityState } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The utility's install, and its id in the install log. */
export const UTILITY = "vmware-utility" satisfies Dependency;

/** HashiCorp's Vagrant VMware Utility, the service VMware's plugin drives VMware through: done
 *  once the core's plan has nothing left for it (the machine check finds it running); installed
 *  from HashiCorp's signed package (the OS asks for an administrator's password). Installed but
 *  stopped: installing again restarts it. `action`: the plan's for it; `first`: it is the first
 *  thing left for Vagrant (the jewel then). */
export function UtilityRequirement({
  utility,
  action,
  first,
  setup,
}: {
  utility: UtilityState;
  action: SetupAction | undefined;
  first: boolean;
  setup: MachineSetupState;
}) {
  const t = useT();
  const own = action?.kind === "manual";
  return (
    <Requirement
      ok={!action}
      title={t("machine.vm.utilityTitle")}
      detail={
        own
          ? t("machine.body.distribution", { name: t("machine.vm.utilityTitle") })
          : utility === "stopped"
            ? t("machine.vm.utilityStopped")
            : t("machine.vm.utilityDetail")
      }
      failed={setup.failure(UTILITY)}
      action={
        own ? (
          <OwnInstall action={action} name={t("machine.vm.utilityTitle")} />
        ) : (
          <Button
            variant={first ? "primary" : "outline"}
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
        )
      }
    />
  );
}
