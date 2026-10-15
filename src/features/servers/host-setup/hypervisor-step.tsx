"use client";

import { Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { KIND, SERVER_KINDS } from "@/features/servers/host-setup/constants";
import { HypervisorMark, MarkTile, Nav, Step } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

export function HypervisorStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, setV, next } = s;
  return (
    <Step icon={Server} title={t("servers.setup.hypervisor.title")} description={t("servers.setup.hypervisor.description")}>
      <RadioList label={t("servers.setup.hypervisor.label")}>
        {SERVER_KINDS.map((p) => (
          <RadioRow
            key={p}
            selected={v.provider === p}
            onSelect={() => {
              setV((s) => ({ ...s, provider: p, port: null }));
            }}
            leading={
              <MarkTile>
                <HypervisorMark provider={p} />
              </MarkTile>
            }
            title={KIND[p].label}
            subtitle={t(`servers.setup.kinds.${p}`)}
          />
        ))}
      </RadioList>
      <Nav right={<Button onClick={next}>{t("servers.setup.nav.continue")}</Button>} />
    </Step>
  );
}
