"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LogoTile } from "@/components/ui/logo-tile";
import { MAIN_PROVIDERS } from "@/features/cloud/cloud-model";
import { cloudLogo } from "@/features/servers/host-setup/constants";
import type { CloudProvider } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** No account connected (none yet, or the last one removed): what the page is for, the main
 *  providers (each a shortcut into the setup on it), and the setup, which offers the others too. */
export function CloudEmptyState({ onSetup, onConnect }: { onSetup: () => void; onConnect: (provider: CloudProvider) => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="cloud"
      title={t("cloud.empty.title")}
      description={t("cloud.empty.description")}
      marks={MAIN_PROVIDERS.map((p) => ({
        name: p.label,
        logo: <LogoTile src={cloudLogo(p.id)} size="sm" />,
        select: {
          label: t("cloud.connect.provider", { provider: p.label }),
          onSelect: () => {
            onConnect(p.id);
          },
        },
      }))}
      action={
        <Button size="sm" onClick={onSetup}>
          <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
        </Button>
      }
    />
  );
}
