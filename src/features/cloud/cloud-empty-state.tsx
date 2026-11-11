"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { providerLogo } from "@/features/servers/provider-glyph";
import { useT } from "@/lib/i18n";
import { providerInitials } from "@/lib/provider-initials";
import { CLOUD_PROVIDERS, PROVIDER_NAMES } from "@/lib/tauri";

/** No account connected (none yet, or the last one removed): what the page is for, every cloud
 *  the launcher runs labs on (the core's list, so one added there shows here), and the setup, where
 *  the provider is picked. */
export function CloudEmptyState({ onSetup }: { onSetup: () => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="cloud"
      title={t("cloud.empty.title")}
      description={t("cloud.empty.description")}
      marks={CLOUD_PROVIDERS.map((id) => ({ name: PROVIDER_NAMES[id], src: providerLogo(id), initials: providerInitials(id) }))}
      action={
        <Button size="sm" onClick={onSetup}>
          <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
        </Button>
      }
    />
  );
}
