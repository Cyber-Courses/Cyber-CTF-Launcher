"use client";

import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import {
  awsCliIdentity,
  awsLogin,
  awsMonthToDateCost,
  awsProfiles,
  azureSubscriptions,
  cloudLogin,
  gcpAccount,
  gcpBillingAccounts,
  gcpOrganizations,
  ociConfig,
  type AzureSubscription,
  type CloudProvider,
  type GcpBillingAccount,
  type GcpOrganization,
  type OciConfig,
  type ServerHostInput,
} from "@/lib/tauri";
import { CLOUD_META } from "@/features/servers/host-setup/constants";
import { useRunLog } from "@/features/servers/host-setup/use-run-log";
import { ignore } from "@/lib/failure";
import { translate } from "@/lib/i18n";

const NONE: never[] = [];

/**
 * What the chosen cloud's CLI knows on this machine, read as the provider is picked so the setup
 * offers choices instead of ids to type: AWS profiles and the signed-in identity (with this
 * month's spend), Azure subscriptions, GCP billing accounts and organizations, and ~/.oci/config.
 * Also the CLI sign-in, with its log.
 */
export function useCloudAccounts(v: ServerHostInput, setV: Dispatch<SetStateAction<ServerHostInput>>, editing: boolean, cloudProvider: CloudProvider) {
  const aws = v.provider === "aws";
  const azure = v.provider === "azure";
  const gcp = v.provider === "gcp";
  const oci = v.provider === "oci";
  const signInLog = useRunLog();
  const [signingIn, setSigningIn] = useState(false);

  // AWS can connect through the CLI (a profile / browser sign-in) or with access keys.
  const [profiles, setProfiles] = useState<string[]>([]);
  const [awsIdentity, setAwsIdentity] = useState<string | null>(null);
  const [checkingId, setCheckingId] = useState(false);
  const [mtdCost, setMtdCost] = useState<number | null>(null);
  useEffect(() => {
    if (aws) awsProfiles().then(setProfiles).catch(ignore("profiles are a convenience; typed by hand otherwise"));
  }, [aws]);
  // Only meaningful in CLI-credentials mode, where the signed-in profile *is* the account.
  // With pasted access keys these would reflect the machine's default AWS chain (a different
  // account), so they are neither fetched nor shown.
  const awsCli = aws && !!v.useCliCreds;
  // Each read is numbered: switching profiles starts a new one, and an older read answering
  // after it must not show the previous profile's account or spend.
  const identityRead = useRef(0);
  const checkAwsIdentity = useCallback((profile: string | null) => {
    const n = ++identityRead.current;
    const latest = () => n === identityRead.current;
    setCheckingId(true);
    awsCliIdentity(profile ?? undefined)
      .then((id) => latest() && setAwsIdentity(id))
      .catch(() => latest() && setAwsIdentity(null))
      .finally(() => latest() && setCheckingId(false));
    awsMonthToDateCost(profile ?? undefined)
      .then((c) => latest() && setMtdCost(c))
      .catch(() => latest() && setMtdCost(null));
  }, []);
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (awsCli) checkAwsIdentity(v.awsProfile ?? null);
  }, [awsCli, v.awsProfile, checkAwsIdentity]);

  // Azure/GCP: the subscriptions/projects the signed-in account can see, so you pick one
  // instead of typing an id.
  const [azureSubs, setAzureSubs] = useState<AzureSubscription[]>([]);
  const [azureChecking, setAzureChecking] = useState(false);
  const loadAzureSubs = useCallback(() => {
    setAzureChecking(true);
    azureSubscriptions()
      .then((subs) => {
        setAzureSubs(subs);
        // Default to the account's default subscription if none chosen yet.
        setV((s) => (s.provider === "azure" && !s.username && subs.length > 0 ? { ...s, username: (subs.find((x) => x.isDefault) ?? subs[0]).id } : s));
      })
      .catch(() => setAzureSubs([]))
      .finally(() => setAzureChecking(false));
  }, [setV]);
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (azure) loadAzureSubs();
  }, [azure, loadAzureSubs]);

  const [gcpBilling, setGcpBilling] = useState<GcpBillingAccount[]>([]);
  const [gcpOrgs, setGcpOrgs] = useState<GcpOrganization[]>([]);
  const [gcpEmail, setGcpEmail] = useState<string | null>(null);
  const [gcpChecking, setGcpChecking] = useState(false);
  const loadGcpProjects = useCallback(() => {
    setGcpChecking(true);
    Promise.all([gcpAccount(), gcpBillingAccounts(), gcpOrganizations()])
      .then(([email, billing, orgs]) => {
        setGcpEmail(email);
        setGcpBilling(billing);
        setGcpOrgs(orgs);
        // Default to the first open billing account if none chosen yet.
        const open = billing.find((b) => b.open) ?? billing[0];
        setV((s) => (s.provider === "gcp" && !s.username && open ? { ...s, username: open.id } : s));
      })
      .catch(() => {
        setGcpEmail(null);
        setGcpBilling([]);
        setGcpOrgs([]);
      })
      .finally(() => setGcpChecking(false));
  }, [setV]);
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (gcp) loadGcpProjects();
  }, [gcp, loadGcpProjects]);

  // OCI: what ~/.oci/config holds, to prefill the compartment (tenancy root) and region.
  const [ociCfg, setOciCfg] = useState<OciConfig | null>(null);
  useEffect(() => {
    if (!oci) return;
    ociConfig()
      .then((cfg) => {
        setOciCfg(cfg);
        setV((s) => (s.provider === "oci" ? { ...s, username: s.username || cfg.tenancy, host: cfg.region && !editing ? cfg.region : s.host } : s));
      })
      .catch(() => setOciCfg(null));
  }, [oci, editing, setV]);

  /** Runs a sign-in, logging it below, with the button busy meanwhile. */
  async function signInWith(cloud: string, task: (onLog: (l: string) => void) => Promise<string>) {
    setSigningIn(true);
    await signInLog.run(translate("servers.setup.credentials.logSigningIn", { cloud }), task);
    setSigningIn(false);
  }

  /** The AWS CLI's browser sign-in for the chosen profile, then who it signed in as. */
  const awsSignIn = () =>
    signInWith("AWS", async (onLog) => {
      await awsLogin(v.awsProfile ?? null, onLog);
      const id = await awsCliIdentity(v.awsProfile ?? undefined);
      setAwsIdentity(id);
      return id ? translate("servers.setup.credentials.logSignedInAs", { id }) : translate("servers.setup.credentials.logNotSignedIn");
    });

  /** The cloud CLI's own sign-in (Azure, GCP), then the subscriptions / projects it now sees. */
  const signIn = () =>
    signInWith(CLOUD_META[cloudProvider].label, async (onLog) => {
      await cloudLogin(cloudProvider, onLog);
      if (cloudProvider === "azure") loadAzureSubs();
      if (cloudProvider === "gcp") loadGcpProjects();
      return translate("servers.setup.credentials.logSignedIn");
    });

  return {
    signingIn,
    signInLog: signInLog.lines,
    signIn,
    awsSignIn,
    profiles,
    // What another provider (or access keys) left behind isn't shown.
    awsIdentity: awsCli ? awsIdentity : null,
    checkingId,
    mtdCost: awsCli ? mtdCost : null,
    azureSubs: azure ? azureSubs : NONE,
    azureChecking,
    gcpBilling: gcp ? gcpBilling : NONE,
    gcpOrgs: gcp ? gcpOrgs : NONE,
    gcpEmail,
    gcpChecking,
    ociCfg: oci ? ociCfg : null,
  };
}
