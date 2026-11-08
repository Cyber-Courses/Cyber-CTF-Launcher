"use client";

import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ChoiceAction } from "@/components/ui/choice-card";
import { BusyLabel, Log, Requirement } from "@/features/machine/setup-steps/parts";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { serverOpenSetup, type SystemReport } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** The cloud path's one step: Terraform on this machine (it creates the labs in the account),
 *  and the way to the Cloud page's account setup, where the account signs in and its CLI is
 *  installed. */
export function CloudStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const { installed, version } = report.terraform;
  return (
    <div className="space-y-3">
      <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
        <Requirement
          ok={installed}
          title="Terraform"
          detail={installed ? (version ?? t("machine.body.installed")) : t("machine.cloud.terraformDetail")}
          action={
            // The step's one jewel while Terraform is missing (Connect stays outline).
            <Button
              variant="primary"
              size="xs"
              onClick={() => {
                setup.install("terraform", "terraform", t("machine.cloud.installLog"));
              }}
              disabled={setup.installing !== null}
            >
              <BusyLabel busy={setup.installing === "terraform"} label={t("machine.vm.installing")} spinner="size-3">
                {t("machine.cloud.installTerraform")}
              </BusyLabel>
            </Button>
          }
        />
      </div>
      <ChoiceAction>
        <span className="text-[0.8125rem] text-muted-foreground">{t("machine.cloud.connectHint")}</span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            serverOpenSetup(null, "cloud").catch(tell(t("machine.cloud.connectFailed")));
          }}
        >
          <ExternalLink className="size-3.5" /> {t("machine.cloud.connect")}
        </Button>
      </ChoiceAction>
      <Log setup={setup} of={["terraform"]} />
    </div>
  );
}
