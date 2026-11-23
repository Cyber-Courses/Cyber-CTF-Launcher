"use client";

import { Cloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { cloudLogo } from "@/features/servers/host-setup/constants";
import { MarkTile, Nav, Step } from "@/features/servers/host-setup/form";
import { ContinueButton } from "@/features/servers/host-setup/step-nav";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";
import { CLOUD_PROVIDERS, PROVIDER_NAMES } from "@/lib/tauri";

export function ProviderStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { onDone, cloudProvider } = s;
  const { pickProvider } = s.form;
  const { next } = s.nav;
  return (
    <Step icon={Cloud} title={t("servers.setup.provider.title")} description={t("servers.setup.provider.description")}>
      <RadioList label={t("servers.setup.provider.label")}>
        {CLOUD_PROVIDERS.map((id) => (
          <RadioRow
            key={id}
            selected={cloudProvider === id}
            onSelect={() => {
              pickProvider(id);
            }}
            leading={
              <MarkTile>
                {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
                <img src={cloudLogo(id)} alt="" className="size-5 object-contain" draggable={false} />
              </MarkTile>
            }
            title={PROVIDER_NAMES[id]}
            subtitle={t(`servers.setup.kinds.${id}`)}
          />
        ))}
      </RadioList>
      <Nav
        left={
          <Button variant="ghost" onClick={onDone}>
            {t("servers.setup.nav.cancel")}
          </Button>
        }
        right={<ContinueButton onClick={next} />}
      />
    </Step>
  );
}
