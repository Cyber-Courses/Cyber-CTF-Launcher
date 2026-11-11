"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useT } from "@/lib/i18n";

/** No account connected (none yet, or the last one removed): what the page is for, and the setup. */
export function CloudEmptyState({ onSetup }: { onSetup: () => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="cloud"
      title={t("cloud.empty.title")}
      description={t("cloud.empty.description")}
      action={
        <Button size="sm" onClick={onSetup}>
          <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
        </Button>
      }
    />
  );
}
