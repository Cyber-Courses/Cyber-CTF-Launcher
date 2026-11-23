"use client";

import { Cloud } from "lucide-react";
import { Field, Select, Step } from "@/features/servers/host-setup/form";
import { BackNav, ContinueButton, StepError } from "@/features/servers/host-setup/step-nav";
import { AccessKeyCredentials, OciCredentials, TokenCredentials } from "@/features/servers/host-setup/credentials-keys";
import { AzureCredentials, GcpCredentials } from "@/features/servers/host-setup/credentials-cli";
import { AwsCliCredentials } from "@/features/servers/host-setup/credentials-aws";
import { REGIONS } from "@/features/servers/host-setup/setup-model";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT, type T } from "@/lib/i18n";
import { PROVIDER_NAMES } from "@/lib/tauri";

/** How the chosen cloud signs in: an OCI config, an API token, the Azure or GCP CLI, the AWS
 *  CLI or AWS access keys; then the region its labs run in. */
export function CredentialsStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { connectionOk } = s;
  const { next, back } = s.nav;
  const { error } = s.save;
  return (
    <Step icon={Cloud} title={stepTitle(s, t)} description={stepDescription(s, t)}>
      <Credentials s={s} />
      <RegionField s={s} />
      <StepError error={error} />
      <BackNav onBack={back}>
        <ContinueButton onClick={next} disabled={!connectionOk} />
      </BackNav>
    </Step>
  );
}

/** The chosen cloud's sign-in. */
function Credentials({ s }: { s: HostSetup }) {
  if (s.traits.oci) return <OciCredentials s={s} />;
  if (s.traits.tokenCloud) return <TokenCredentials s={s} />;
  if (s.traits.azure) return <AzureCredentials s={s} />;
  if (s.traits.gcp) return <GcpCredentials s={s} />;
  return s.form.v.useCliCreds ? <AwsCliCredentials s={s} /> : <AccessKeyCredentials s={s} />;
}

/** Where the labs run: the cloud's regions, and a saved one the list lacks shown as itself. */
function RegionField({ s }: { s: HostSetup }) {
  const t = useT();
  const { v } = s.form;
  const regions = REGIONS[s.cloudProvider];
  return (
    <div className="mt-3">
      <Field label={s.traits.azure ? t("servers.setup.credentials.location") : t("servers.setup.credentials.region")}>
        <Select
          value={v.host}
          onChange={(e) => {
            s.form.set("host", e.target.value);
          }}
        >
          {v.host && !regions.some(([code]) => code === v.host) && <option value={v.host}>{v.host}</option>}
          {regions.map(([code, name]) => (
            <option key={code} value={code}>
              {code} · {name}
            </option>
          ))}
        </Select>
      </Field>
    </div>
  );
}

const tokenCloudName = (s: HostSetup) => PROVIDER_NAMES[s.traits.digitalocean ? "digitalocean" : "linode"];

function stepTitle(s: HostSetup, t: T) {
  if (s.traits.azure) return t("servers.setup.credentials.titles.azure");
  if (s.traits.gcp) return t("servers.setup.credentials.titles.gcp");
  if (s.traits.digitalocean) return t("servers.setup.credentials.titles.digitalocean");
  if (s.traits.linode) return t("servers.setup.credentials.titles.linode");
  if (s.traits.oci) return PROVIDER_NAMES.oci;
  return s.form.v.useCliCreds ? "AWS CLI" : t("servers.setup.credentials.titles.accessKeys");
}

function stepDescription(s: HostSetup, t: T) {
  if (s.traits.azure) return t("servers.setup.credentials.descriptions.azure");
  if (s.traits.gcp) return t("servers.setup.credentials.descriptions.gcp");
  if (s.traits.tokenCloud) return t("servers.setup.credentials.descriptions.token", { cloud: tokenCloudName(s) });
  if (s.traits.oci) return t("servers.setup.credentials.descriptions.oci");
  return s.form.v.useCliCreds ? t("servers.setup.credentials.descriptions.awsCli") : t("servers.setup.credentials.descriptions.accessKeys");
}
