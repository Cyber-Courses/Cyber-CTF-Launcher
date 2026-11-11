"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useOpenSetup } from "@/features/machine/use-open-setup";
import { useT } from "@/lib/i18n";

/** No container engine or hypervisor installed here: Add a provider (the provider picker), or run
 *  labs on a server instead. */
export function MachineEmptyState({ onUseServer }: { onUseServer: () => void }) {
  const t = useT();
  const openSetup = useOpenSetup();
  return (
    <EmptyState
      icon="machine"
      title={t("machine.empty.title")}
      description={t("machine.empty.description")}
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
