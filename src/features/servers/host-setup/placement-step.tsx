"use client";

import { HardDrive } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Field, Input, Note, Step } from "@/features/servers/host-setup/form";
import { ProxmoxPlacementFields } from "@/features/servers/host-setup/proxmox-placement";
import { BackNav, SaveAndTestButton, StepError } from "@/features/servers/host-setup/step-nav";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

export function PlacementStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, saving, error, set, text, back, saveAndTest } = s;
  return (
    <Step icon={HardDrive} title={t("servers.setup.placement.title")} description={t("servers.setup.placement.description")}>
      {v.provider === "proxmox" ? (
        <ProxmoxPlacementFields s={s} />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label={t("servers.setup.placement.datastore")} hint={t("servers.setup.placement.optional")}>
            <Input {...text("datastore")} placeholder="datastore1" />
          </Field>
          <Field label={t("servers.setup.placement.portGroup")} hint={t("servers.setup.placement.optional")}>
            <Input {...text("network")} placeholder="VM Network" />
          </Field>
        </div>
      )}
      {v.provider === "proxmox" && (
        <Note className="mt-4 flex items-center gap-3">
          <span className="min-w-0 flex-1">
            <span className="block text-[0.8125rem] text-foreground">{t("servers.setup.placement.selfSigned")}</span>
            <span className="block text-[0.75rem] text-muted-foreground">{t("servers.setup.placement.selfSignedHint")}</span>
          </span>
          <Switch
            checked={v.insecureTls ?? false}
            onCheckedChange={(c) => {
              set("insecureTls", c);
            }}
            aria-label={t("servers.setup.placement.selfSigned")}
          />
        </Note>
      )}
      <StepError error={error} />
      <BackNav onBack={back}>
        <SaveAndTestButton onClick={saveAndTest} saving={saving} />
      </BackNav>
    </Step>
  );
}
