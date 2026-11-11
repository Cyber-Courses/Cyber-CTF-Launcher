"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState, type EmptyStateMark } from "@/components/ui/empty-state";
import { EngineLogo } from "@/features/machine/engine-logo";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { localHypervisors, providerLabel } from "@/features/machine/hypervisors";
import { enginesFor } from "@/features/machine/setup-steps/engines";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import type { SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** What Add a provider offers on this machine, nothing being installed: the container engines of
 *  its OS, then the hypervisors it can use. */
const offered = (report: SystemReport): EmptyStateMark[] => [
  ...enginesFor(report).map((e) => ({ name: e.name, logo: <EngineLogo engine={e} size="sm" /> })),
  ...localHypervisors(report).map((p) => ({ name: providerLabel(p, report.os), logo: <HypervisorLogo provider={p.provider} size="sm" /> })),
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
