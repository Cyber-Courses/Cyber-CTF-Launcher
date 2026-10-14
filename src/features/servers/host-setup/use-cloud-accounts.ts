"use client";

import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from "react";
import {
  awsCliIdentity,
  awsLogin,
  awsMonthToDateCost,
  awsProfiles,
  azureSubscriptions,
  cloudLogin,
  cloudLoginCancel,
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
import { describeError } from "@/lib/errors";
import { useLatest } from "@/lib/use-latest";
import { translate } from "@/lib/i18n";

const NONE: never[] = [];

type SetInput = Dispatch<SetStateAction<ServerHostInput>>;

/**
 * What the chosen cloud's CLI knows on this machine, read as the provider is picked so the setup
 * offers choices instead of ids to type: AWS profiles and the signed-in identity (with this
 * month's spend), Azure subscriptions, GCP billing accounts and organizations, and ~/.oci/config.
 * Also the CLI sign-in, with its log.
 */
export function useCloudAccounts(v: ServerHostInput, setV: SetInput, editing: boolean, cloudProvider: CloudProvider) {
  const aws = v.provider === "aws";
  const azure = v.provider === "azure";
  const gcp = v.provider === "gcp";
  const oci = v.provider === "oci";
  const signInLog = useRunLog();
  const [signingIn, setSigningIn] = useState(false);

  // Only meaningful in CLI-credentials mode, where the signed-in profile *is* the account.
  // With pasted access keys these would reflect the machine's default AWS chain (a different
  // account), so they are neither fetched nor shown.
  const awsCli = aws && !!v.useCliCreds;
  const awsAccount = useAwsAccount(aws, awsCli, v.awsProfile ?? null);
  const azureAccount = useAzureSubscriptions(azure, setV);
  const gcpAccounts = useGcpAccounts(gcp, setV);
  const ociCfg = useOciConfig(oci, editing, setV);

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
      awsAccount.setAwsIdentity(id);
      return id ? translate("servers.setup.credentials.logSignedInAs", { id }) : translate("servers.setup.credentials.logNotSignedIn");
    });

  /** The cloud CLI's own sign-in (Azure, GCP), then the subscriptions / projects it now sees.
   *  The other clouds sign in with a token or a config file: `cloud_login` refuses them. */
  const signIn = async () => {
    const cloud = cloudProvider;
    if (cloud !== "azure" && cloud !== "gcp") return;
    await signInWith(CLOUD_META[cloud].label, async (onLog) => {
      await cloudLogin(cloud, onLog);
      if (cloud === "azure") azureAccount.loadAzureSubs();
      if (cloud === "gcp") gcpAccounts.loadGcpProjects();
      return translate("servers.setup.credentials.logSignedIn");
    });
  };

  /** Stops the sign-in in progress: its CLI is stopped, and the log says it was cancelled. */
  const cancelSignIn = () => void cloudLoginCancel().catch(ignore("the sign-in ends on its own within its time limit"));

  return {
    signingIn,
    signInLog: signInLog.lines,
    signIn,
    cancelSignIn,
    awsSignIn,
    profiles: awsAccount.profiles,
    // What another provider (or access keys) left behind isn't shown.
    awsIdentity: awsCli ? awsAccount.awsIdentity : null,
    checkingId: awsAccount.checkingId,
    mtdCost: awsCli ? awsAccount.mtdCost : null,
    azureSubs: azure ? azureAccount.azureSubs : NONE,
    azureChecking: azureAccount.azureChecking,
    gcpBilling: gcp ? gcpAccounts.gcpBilling : NONE,
    gcpOrgs: gcp ? gcpAccounts.gcpOrgs : NONE,
    gcpEmail: gcpAccounts.gcpEmail,
    gcpChecking: gcpAccounts.gcpChecking,
    gcpError: gcp ? gcpAccounts.gcpError : null,
    ociCfg: oci ? ociCfg : null,
  };
}

/** AWS, which connects through the CLI (a profile / browser sign-in) or with access keys: the
 *  CLI's profiles, and in CLI mode the profile's signed-in identity and this month's spend. */
function useAwsAccount(aws: boolean, awsCli: boolean, awsProfile: string | null) {
  const [profiles, setProfiles] = useState<string[]>([]);
  const [awsIdentity, setAwsIdentity] = useState<string | null>(null);
  const [checkingId, setCheckingId] = useState(false);
  const [mtdCost, setMtdCost] = useState<number | null>(null);
  useEffect(() => {
    if (aws) awsProfiles().then(setProfiles).catch(ignore("profiles are a convenience; typed by hand otherwise"));
  }, [aws]);
  // Only the newest read applies: switching profiles starts a new one, and an older read answering
  // after it must not show the previous profile's account or spend.
  const beginIdentityRead = useLatest();
  const checkAwsIdentity = useCallback(
    (profile: string | null) => {
      const latest = beginIdentityRead();
      setCheckingId(true);
      awsCliIdentity(profile ?? undefined)
        .then((id) => latest() && setAwsIdentity(id))
        .catch(() => latest() && setAwsIdentity(null))
        .finally(() => latest() && setCheckingId(false));
      awsMonthToDateCost(profile ?? undefined)
        .then((c) => latest() && setMtdCost(c))
        .catch(() => latest() && setMtdCost(null));
    },
    [beginIdentityRead],
  );
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (awsCli) checkAwsIdentity(awsProfile);
  }, [awsCli, awsProfile, checkAwsIdentity]);
  return { profiles, awsIdentity, setAwsIdentity, checkingId, mtdCost };
}

/** Azure: the subscriptions the signed-in account can see, so you pick one instead of typing an
 *  id. The account's default subscription is chosen if none is yet. */
function useAzureSubscriptions(azure: boolean, setV: SetInput) {
  const [azureSubs, setAzureSubs] = useState<AzureSubscription[]>([]);
  const [azureChecking, setAzureChecking] = useState(false);
  // Numbered as the AWS reads are: a sign-in reads again, and an older read answering after it
  // must not show the subscriptions from before.
  const beginSubsRead = useLatest();
  const loadAzureSubs = useCallback(() => {
    const latest = beginSubsRead();
    setAzureChecking(true);
    azureSubscriptions()
      .then((subs) => {
        if (!latest()) return;
        setAzureSubs(subs);
        // Default to the account's default subscription if none chosen yet.
        setV((s) => (s.provider === "azure" && !s.username && subs.length > 0 ? { ...s, username: (subs.find((x) => x.isDefault) ?? subs[0]).id } : s));
      })
      .catch(() => latest() && setAzureSubs([]))
      .finally(() => latest() && setAzureChecking(false));
  }, [setV, beginSubsRead]);
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (azure) loadAzureSubs();
  }, [azure, loadAzureSubs]);
  return { azureSubs, azureChecking, loadAzureSubs };
}

/** GCP: the signed-in account, its billing accounts and organizations. The first open billing
 *  account is chosen if none is yet. */
function useGcpAccounts(gcp: boolean, setV: SetInput) {
  const [gcpBilling, setGcpBilling] = useState<GcpBillingAccount[]>([]);
  const [gcpOrgs, setGcpOrgs] = useState<GcpOrganization[]>([]);
  const [gcpEmail, setGcpEmail] = useState<string | null>(null);
  const [gcpChecking, setGcpChecking] = useState(false);
  const [gcpError, setGcpError] = useState<string | null>(null);
  // Numbered as the AWS reads are (see there).
  const beginProjectsRead = useLatest();
  const loadGcpProjects = useCallback(() => {
    const latest = beginProjectsRead();
    setGcpChecking(true);
    // Each list on its own: one call failing (no permission to list organizations, say) keeps
    // what the others found, and says what went wrong.
    void Promise.allSettled([gcpAccount(), gcpBillingAccounts(), gcpOrganizations()])
      .then(([email, billing, orgs]) => {
        if (!latest()) return;
        setGcpEmail(email.status === "fulfilled" ? email.value : null);
        const accounts = billing.status === "fulfilled" ? billing.value : [];
        setGcpBilling(accounts);
        setGcpOrgs(orgs.status === "fulfilled" ? orgs.value : []);
        const failed = [email, billing, orgs].find((r) => r.status === "rejected");
        setGcpError(failed ? describeError(failed.reason) : null);
        // Default to the first open billing account if none chosen yet.
        const open = accounts.find((b) => b.open) ?? accounts[0];
        setV((s) => (s.provider === "gcp" && !s.username && open ? { ...s, username: open.id } : s));
      })
      .finally(() => latest() && setGcpChecking(false));
  }, [setV, beginProjectsRead]);
  useEffect(() => {
    // A read starting shows its "checking" state at once.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (gcp) loadGcpProjects();
  }, [gcp, loadGcpProjects]);
  return { gcpBilling, gcpOrgs, gcpEmail, gcpChecking, gcpError, loadGcpProjects };
}

/** OCI: what ~/.oci/config holds, to prefill the compartment (tenancy root) and region. */
function useOciConfig(oci: boolean, editing: boolean, setV: SetInput) {
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
  return ociCfg;
}
