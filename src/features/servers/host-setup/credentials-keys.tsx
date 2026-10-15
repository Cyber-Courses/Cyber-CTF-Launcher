"use client";

import { CheckCircle2 } from "lucide-react";
import { Field, Input, Note } from "@/features/servers/host-setup/form";
import { ExternalLinkButton, SecretInput } from "@/features/servers/host-setup/credentials-parts";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

// The clouds that sign in with a key or token kept on this machine: Oracle Cloud's API key,
// a DigitalOcean or Linode API token, an AWS access key pair.

/** Oracle Cloud: the API key in ~/.oci/config, and the compartment labs go in. */
export function OciCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  return (
    <div className="space-y-3">
      <Note className="flex flex-wrap items-center gap-2">
        {s.ociCfg?.configured ? (
          <span className="flex items-center gap-1.5 text-success">
            <CheckCircle2 className="size-3.5" /> {t("servers.setup.credentials.ociFound")}
          </span>
        ) : (
          <span className="text-muted-foreground">{t("servers.setup.credentials.ociMissing")}</span>
        )}
        <ExternalLinkButton url="https://docs.oracle.com/en-us/iaas/Content/API/Concepts/apisigningkey.htm" className="ml-auto">
          {t("servers.setup.credentials.howToSetUp")}
        </ExternalLinkButton>
      </Note>
      <Field label={t("servers.setup.credentials.compartment")} hint={t("servers.setup.credentials.compartmentHint")}>
        <Input {...s.text("username")} placeholder="ocid1.compartment.oc1..…" />
      </Field>
    </div>
  );
}

/** DigitalOcean and Linode: a pasted API token. */
export function TokenCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  return (
    <div className="space-y-3">
      <Field label={t("servers.setup.credentials.apiToken")} hint={t("servers.setup.credentials.apiTokenHint")}>
        <SecretInput s={s} placeholder={s.digitalocean ? "dop_v1_…" : "…"} />
      </Field>
      <p className="text-[0.75rem] text-muted-foreground">
        {t("servers.setup.credentials.noToken")}{" "}
        <ExternalLinkButton url={s.digitalocean ? "https://cloud.digitalocean.com/account/api/tokens" : "https://cloud.linode.com/profile/tokens"}>
          {t("servers.setup.credentials.createToken", { cloud: s.digitalocean ? "DigitalOcean" : "Linode" })}
        </ExternalLinkButton>
      </p>
    </div>
  );
}

/** AWS with an access key pair (the secret goes to the OS keychain). */
export function AccessKeyCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  return (
    <>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={t("servers.setup.credentials.accessKeyId")} hint={t("servers.setup.credentials.accessKeyHint")}>
          <Input {...s.text("username")} placeholder="AKIA…" />
        </Field>
        <Field label={t("servers.setup.credentials.secretKey")} hint={t("servers.setup.connection.keychainHint")}>
          <SecretInput s={s} />
        </Field>
      </div>
      <p className="mt-1.5 text-[0.75rem] text-muted-foreground">
        {t("servers.setup.credentials.noKeys")}{" "}
        <ExternalLinkButton url="https://console.aws.amazon.com/iam/home#/security_credentials">{t("servers.setup.credentials.createKeys")}</ExternalLinkButton>
      </p>
    </>
  );
}
