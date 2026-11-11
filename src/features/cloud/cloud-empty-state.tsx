"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { MAIN_PROVIDERS } from "@/features/cloud/cloud-model";
import { cloudLogo } from "@/features/servers/host-setup/constants";
import { useT } from "@/lib/i18n";

/** No account connected (none yet, or the last one removed): what the page is for, the main
 *  providers' logos, and the setup, where the provider is picked (these or the others). */
export function CloudEmptyState({ onSetup }: { onSetup: () => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="cloud"
      title={t("cloud.empty.title")}
      description={t("cloud.empty.description")}
      marks={MAIN_PROVIDERS.map((p) => ({ name: p.label, src: cloudLogo(p.id) }))}
      action={
        <Button size="sm" onClick={onSetup}>
          <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
        </Button>
      }
    />
  );
}
