"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useT } from "@/lib/i18n";

export function ServersEmptyState({ onAdd }: { onAdd: () => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="server"
      title={t("servers.empty.title")}
      description={t("servers.empty.description")}
      action={
        <Button size="sm" onClick={onAdd}>
          <Plus className="size-3.5" /> {t("servers.screen.addServer")}
        </Button>
      }
    />
  );
}
