"use client";

import { ArrowLeft, HardDrive } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { Field, Input, Nav, Note, Step } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

export function PlacementStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, saving, error, set, text, back, saveAndTest } = s;
  return (
    <Step icon={HardDrive} title={t("servers.setup.placement.title")} description={t("servers.setup.placement.description")}>
      <div className={cn("grid gap-3", v.provider === "proxmox" ? "sm:grid-cols-3" : "sm:grid-cols-2")}>
        {v.provider === "proxmox" && (
          <Field label={t("servers.setup.placement.node")} hint={t("servers.setup.placement.optional")}>
            <Input {...text("node")} placeholder="pve" />
          </Field>
        )}
        <Field
          label={v.provider === "proxmox" ? t("servers.setup.placement.storage") : t("servers.setup.placement.datastore")}
          hint={t("servers.setup.placement.optional")}
        >
          <Input {...text("datastore")} placeholder={v.provider === "proxmox" ? "local-lvm" : "datastore1"} />
        </Field>
        <Field
          label={v.provider === "proxmox" ? t("servers.setup.placement.bridge") : t("servers.setup.placement.portGroup")}
          hint={t("servers.setup.placement.optional")}
        >
          <Input {...text("network")} placeholder={v.provider === "proxmox" ? "vmbr0" : "VM Network"} />
        </Field>
      </div>
      {v.provider === "proxmox" && (
        <Note className="mt-4 flex items-center gap-3">
          <span className="min-w-0 flex-1">
            <span className="block text-[0.8125rem] text-foreground">{t("servers.setup.placement.selfSigned")}</span>
            <span className="block text-[0.75rem] text-muted-foreground">{t("servers.setup.placement.selfSignedHint")}</span>
          </span>
          <Switch checked={v.insecureTls ?? false} onCheckedChange={(c) => set("insecureTls", c)} aria-label={t("servers.setup.placement.selfSigned")} />
        </Note>
      )}
      {error && (
        <Note className="mt-3 flex items-start gap-2.5">
          <StatusDot tone="fail" className="mt-1.5" />
          <span className="min-w-0 break-words text-muted-foreground">{error}</span>
        </Note>
      )}
      <Nav
        left={
          <Button variant="ghost" onClick={back}>
            <ArrowLeft className="size-4" /> {t("servers.setup.nav.back")}
          </Button>
        }
        right={
          <Button onClick={saveAndTest} disabled={saving}>
            {saving && <Spinner className="size-4" />} {t("servers.setup.nav.saveAndTest")}
          </Button>
        }
      />
    </Step>
  );
}
