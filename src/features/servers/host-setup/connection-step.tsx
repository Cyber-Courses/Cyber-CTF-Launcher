"use client";

import { useEffect, useState } from "react";
import { ArrowLeft, Check, Copy, KeyRound, Network } from "lucide-react";
import { Button } from "@/components/ui/button";
import { KIND } from "@/features/servers/host-setup/constants";
import { Field, Input, Nav, Note, Step } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { serverPublicKey } from "@/lib/tauri/servers";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";

export function ConnectionStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, secretKept, kind, set, text, connectionOk, next, back } = s;
  // Proxmox accepts an API token id (user@realm!name) in the username field; in that mode the
  // "password" is the token secret and SSH uses the launcher's key, which must be authorized
  // on the node, so we show it.
  const isToken = v.provider === "proxmox" && (v.username ?? "").includes("!");
  const [pubkey, setPubkey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (isToken && pubkey === null) serverPublicKey().then(setPubkey).catch(warn("reading this machine's public key"));
  }, [isToken, pubkey]);
  const copyKey = async () => {
    if (!pubkey) return;
    try {
      await navigator.clipboard.writeText(pubkey);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard blocked: the key is shown for manual copy */
    }
  };
  return (
    <Step
      icon={Network}
      title={t("servers.setup.connection.title", { kind: KIND[v.provider].label })}
      description={v.provider === "proxmox" ? t("servers.setup.connection.descriptionProxmox") : t("servers.setup.connection.descriptionSsh")}
    >
      <div className="grid gap-3 sm:grid-cols-[1fr_1fr_6.875rem]">
        <Field label={t("servers.setup.connection.name")}>
          <Input
            mono={false}
            {...text("name")}
            placeholder={v.provider === "proxmox" ? t("servers.setup.connection.namePlaceholderProxmox") : t("servers.setup.connection.namePlaceholderEsxi")}
          />
        </Field>
        <Field label={t("servers.setup.connection.host")}>
          <Input {...text("host")} placeholder={t("servers.setup.connection.hostPlaceholder")} />
        </Field>
        <Field label={v.provider === "proxmox" ? t("servers.setup.connection.apiPort") : t("servers.setup.connection.sshPort")}>
          <Input
            type="number"
            min={1}
            max={65535}
            value={v.port ?? ""}
            onChange={(e) => set("port", e.target.value ? Number(e.target.value) : null)}
            placeholder={String(kind.port)}
          />
        </Field>
      </div>
      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <Field label={t("servers.setup.connection.username")} hint={v.provider === "proxmox" ? t("servers.setup.connection.usernameHint") : undefined}>
          <Input {...text("username")} placeholder={kind.user} />
        </Field>
        <Field
          label={isToken ? t("servers.setup.connection.tokenSecret") : t("servers.setup.connection.password")}
          hint={t("servers.setup.connection.keychainHint")}
        >
          <Input
            type="password"
            value={v.password ?? ""}
            onChange={(e) => set("password", e.target.value || null)}
            placeholder={secretKept ? t("servers.setup.connection.keepPlaceholder") : ""}
            autoComplete="off"
          />
        </Field>
      </div>
      {isToken && (
        <Note className="mt-3">
          <div className="flex items-center gap-1.5 text-[0.8125rem] font-medium">
            <KeyRound className="size-3.5 text-muted-foreground" /> {t("servers.setup.connection.authorizeKey")}
          </div>
          <p className="mt-1 text-[0.75rem] text-muted-foreground">
            {t.rich("servers.setup.connection.authorizeKeyBody", {
              code: (c) => <code className="mx-1 rounded-xs bg-glass-2 px-1 py-0.5 font-mono">{c}</code>,
            })}
          </p>
          <div className="mt-2 flex items-start gap-2">
            <code className="surface-log min-w-0 flex-1 break-all rounded-sm px-2 py-1.5 font-mono text-[0.6875rem] text-muted-foreground">
              {pubkey ?? t("servers.setup.connection.loadingKey")}
            </code>
            <Button variant="outline" size="xs" onClick={copyKey} disabled={!pubkey}>
              {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3" />}{" "}
              {copied ? t("servers.setup.connection.copied") : t("servers.setup.connection.copy")}
            </Button>
          </div>
        </Note>
      )}
      <Nav
        left={
          <Button variant="ghost" onClick={back}>
            <ArrowLeft className="size-4" /> {t("servers.setup.nav.back")}
          </Button>
        }
        right={
          <Button onClick={next} disabled={!connectionOk}>
            {t("servers.setup.nav.continue")}
          </Button>
        }
      />
    </Step>
  );
}
