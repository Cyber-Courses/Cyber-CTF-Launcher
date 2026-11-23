"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { providerLogo } from "@/features/servers/provider-glyph";
import { providerInitials } from "@/lib/provider-initials";
import { PROVIDER_NAMES, SERVER_PROVIDERS } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** No server saved yet: what the page is for, the servers it takes (Proxmox, VMware ESXi), and
 *  Add server. */
export function ServersEmptyState({ onAdd }: { onAdd: () => void }) {
  const t = useT();
  return (
    <EmptyState
      icon="server"
      title={t("servers.empty.title")}
      description={t("servers.empty.description")}
      marks={SERVER_PROVIDERS.map((id) => ({ name: PROVIDER_NAMES[id], src: providerLogo(id), initials: providerInitials(id) }))}
      action={
        <Button size="sm" onClick={onAdd}>
          <Plus className="size-3.5" /> {t("servers.screen.addServer")}
        </Button>
      }
    />
  );
}
