"use client";

import { Cloud } from "lucide-react";
import { Field, Select, Step } from "@/features/servers/host-setup/form";
import { BackNav, ContinueButton, StepError } from "@/features/servers/host-setup/step-nav";
import { AccessKeyCredentials, OciCredentials, TokenCredentials } from "@/features/servers/host-setup/credentials-keys";
import { AwsCliCredentials, AzureCredentials, GcpCredentials } from "@/features/servers/host-setup/credentials-cli";
import { REGIONS } from "@/features/servers/host-setup/setup-model";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT, type T } from "@/lib/i18n";

/** How the chosen cloud signs in: an OCI config, an API token, the Azure or GCP CLI, the AWS
 *  CLI or AWS access keys; then the region its labs run in. */
export function CredentialsStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { error, connectionOk, next, back } = s;
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
  if (s.oci) return <OciCredentials s={s} />;
  if (s.tokenCloud) return <TokenCredentials s={s} />;
  if (s.azure) return <AzureCredentials s={s} />;
  if (s.gcp) return <GcpCredentials s={s} />;
  return s.v.useCliCreds ? <AwsCliCredentials s={s} /> : <AccessKeyCredentials s={s} />;
}

/** Where the labs run: the cloud's regions, and a saved one the list lacks shown as itself. */
function RegionField({ s }: { s: HostSetup }) {
  const t = useT();
  const { v } = s;
  const regions = REGIONS[s.cloudProvider];
  return (
    <div className="mt-3">
      <Field label={s.azure ? t("servers.setup.credentials.location") : t("servers.setup.credentials.region")}>
        <Select
          value={v.host}
          onChange={(e) => {
            s.set("host", e.target.value);
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

const tokenCloudName = (s: HostSetup) => (s.digitalocean ? "DigitalOcean" : "Linode");

function stepTitle(s: HostSetup, t: T) {
  if (s.azure) return t("servers.setup.credentials.titles.azure");
  if (s.gcp) return t("servers.setup.credentials.titles.gcp");
  if (s.digitalocean) return t("servers.setup.credentials.titles.digitalocean");
  if (s.linode) return t("servers.setup.credentials.titles.linode");
  if (s.oci) return "Oracle Cloud";
  return s.v.useCliCreds ? "AWS CLI" : t("servers.setup.credentials.titles.accessKeys");
}

function stepDescription(s: HostSetup, t: T) {
  if (s.azure) return t("servers.setup.credentials.descriptions.azure");
  if (s.gcp) return t("servers.setup.credentials.descriptions.gcp");
  if (s.tokenCloud) return t("servers.setup.credentials.descriptions.token", { cloud: tokenCloudName(s) });
  if (s.oci) return t("servers.setup.credentials.descriptions.oci");
  return s.v.useCliCreds ? t("servers.setup.credentials.descriptions.awsCli") : t("servers.setup.credentials.descriptions.accessKeys");
}
