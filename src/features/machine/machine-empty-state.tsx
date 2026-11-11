"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { AvatarItem } from "@/components/ui/avatar";
import { EmptyState } from "@/components/ui/empty-state";
import { hypervisorLogo } from "@/features/machine/hypervisor-logo";
import { localHypervisors, providerLabel } from "@/features/machine/hypervisors";
import { enginesFor } from "@/features/machine/setup-steps/engines";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { providerInitials } from "@/lib/provider-initials";
import type { SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** What Add a provider offers on this machine, nothing being installed: the container engines of
 *  its OS, then the hypervisors it can use. */
const offered = (report: SystemReport): AvatarItem[] => [
  ...enginesFor(report).map((e) => ({ name: e.name, src: e.logo, fill: e.tile })),
  ...localHypervisors(report).map((p) => ({
    name: providerLabel(p, report.os),
    src: hypervisorLogo(p.provider),
    initials: providerInitials(p.provider, report.os),
  })),
];

/** No container engine or hypervisor installed here: the providers this machine can take, Add a
 *  provider (the provider picker), or run labs on a server instead. */
export function MachineEmptyState({ report, onUseServer }: { report: SystemReport; onUseServer: () => void }) {
  const t = useT();
  const openSetup = useOpenSetup();
  return (
    <EmptyState
      icon="machine"
      title={t("machine.empty.title")}
      description={t("machine.empty.description")}
      marks={offered(report)}
      action={
        <Button
          size="sm"
          onClick={() => {
            openSetup("add");
          }}
        >
          <Plus className="size-3.5" /> {t("machine.screen.addProvider")}
        </Button>
      }
      secondary={
        <Button variant="link" onClick={onUseServer}>
          {t("machine.screen.useServer")}
        </Button>
      }
    />
  );
}
