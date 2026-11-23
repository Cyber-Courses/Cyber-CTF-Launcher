"use client";

import { useCallback, useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { useCliSignIn } from "@/features/servers/host-setup/use-cli-sign-in";
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
  PROVIDER_NAMES,
  type AzureSubscription,
  type CloudProvider,
  type GcpBillingAccount,
  type GcpOrganization,
  type OciConfig,
  type ServerHostInput,
} from "@/lib/tauri";
import { ignore, warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";
import { useLatest } from "@/lib/use-latest";
import { translate } from "@/lib/i18n";

const NONE: never[] = [];

type SetInput = Dispatch<SetStateAction<ServerHostInput>>;

/**
 * What the chosen cloud's CLI knows on this machine, read as the provider is picked so the setup
 * offers choices instead of ids to type: AWS profiles and the signed-in identity (with this
 * month's spend), Azure subscriptions, GCP billing accounts and organizations, and ~/.oci/config.
 * Also the CLI sign-in, the same for every cloud (`useCliSignIn`).
 */
export function useCloudAccounts(v: ServerHostInput, setV: SetInput, editing: boolean, cloudProvider: CloudProvider) {
  const aws = v.provider === "aws";
  const azure = v.provider === "azure";
  const gcp = v.provider === "gcp";
  const oci = v.provider === "oci";
  const cliSignIn = useCliSignIn();

  // Only meaningful in CLI-credentials mode, where the signed-in profile *is* the account.
  // With pasted access keys these would reflect the machine's default AWS chain (a different
  // account), so they are neither fetched nor shown.
  const awsCli = aws && !!v.useCliCreds;
  const awsAccount = useAwsAccount(aws, awsCli, v.awsProfile ?? null);
  const azureAccount = useAzureSubscriptions(azure, setV);
  const gcpAccounts = useGcpAccounts(gcp, setV);
  const ociCfg = useOciConfig(oci, editing, setV);

  /** The AWS CLI's browser sign-in for the chosen profile, then who it signed in as. */
  const awsSignIn = () => {
    void cliSignIn.run(PROVIDER_NAMES.aws, async (onLog) => {
      await awsLogin(v.awsProfile ?? null, onLog);
      const id = await awsCliIdentity(v.awsProfile ?? undefined);
      awsAccount.setAwsIdentity(id);
      return id ? translate("servers.setup.credentials.logSignedInAs", { id }) : translate("servers.setup.credentials.logNotSignedIn");
    });
  };

  /** The cloud CLI's own sign-in (Azure, GCP), then the subscriptions / projects it now sees.
   *  The other clouds sign in with a token or a config file: `cloud_login` refuses them. */
  const signIn = () => {
    const cloud = cloudProvider;
    if (cloud !== "azure" && cloud !== "gcp") return;
    void cliSignIn.run(PROVIDER_NAMES[cloud], async (onLog) => {
      await cloudLogin(cloud, onLog);
      if (cloud === "azure") azureAccount.loadAzureSubs();
      else gcpAccounts.loadGcpProjects();
      return translate("servers.setup.credentials.logSignedIn");
    });
  };

  return {
    signingIn: cliSignIn.running,
    signInState: cliSignIn.state,
    signInLog: cliSignIn.lines,
    signIn,
    cancelSignIn: cliSignIn.cancel,
    awsSignIn,
    profiles: awsAccount.profiles,
    // What another provider (or access keys) left behind isn't shown.
    awsIdentity: awsCli ? awsAccount.awsIdentity : null,
    checkingId: awsAccount.checkingId,
    mtdCost: awsCli ? awsAccount.mtdCost : null,
    azureSubs: azure ? azureAccount.azureSubs : NONE,
    azureChecking: azureAccount.azureChecking,
    azureError: azure ? azureAccount.azureError : null,
    gcpBilling: gcp ? gcpAccounts.gcpBilling : NONE,
    gcpOrgs: gcp ? gcpAccounts.gcpOrgs : NONE,
    gcpEmail: gcpAccounts.gcpEmail,
    gcpChecking: gcpAccounts.gcpChecking,
    gcpError: gcp ? gcpAccounts.gcpError : null,
    ociCfg: oci ? ociCfg : null,
  };
}

/** A read of what a cloud's CLI knows (`read`, which never fails: it answers what it couldn't
 *  read as nothing, or as its error), applied by `apply`: run when `enabled` turns on or `read`
 *  changes, and again by `reload`, `checking` while it runs. Only the newest read applies: a
 *  sign-in or another profile reads again, and an older read answering after it must not show
 *  what was there before. */
function useCliRead<T>(enabled: boolean, read: () => Promise<T>, apply: (answer: T) => void) {
  const [checking, setChecking] = useState(false);
  const beginRead = useLatest();
  const reload = useCallback(() => {
    const latest = beginRead();
    setChecking(true);
    void read()
      .then((answer) => {
        if (latest()) apply(answer);
      })
      .finally(() => {
        if (latest()) setChecking(false);
      });
  }, [beginRead, read, apply]);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- a read starting shows its "checking" state at once
    if (enabled) reload();
  }, [enabled, reload]);
  return { checking, reload };
}

/** AWS, which connects through the CLI (a profile / browser sign-in) or with access keys: the
 *  CLI's profiles, and in CLI mode the profile's signed-in identity and this month's spend. */
function useAwsAccount(aws: boolean, awsCli: boolean, awsProfile: string | null) {
  const [profiles, setProfiles] = useState<string[]>([]);
  const [awsIdentity, setAwsIdentity] = useState<string | null>(null);
  const [mtdCost, setMtdCost] = useState<number | null>(null);
  useEffect(() => {
    if (aws) awsProfiles().then(setProfiles).catch(ignore("profiles are a convenience; typed by hand otherwise"));
  }, [aws]);
  // The identity and the spend each read as nothing when they can't be read; switching profiles
  // reads both again.
  const readIdentity = useCallback(
    () => awsCliIdentity(awsProfile ?? undefined).catch(ignore("the profile isn't signed in: no account shown", () => null)),
    [awsProfile],
  );
  const readCost = useCallback(
    () => awsMonthToDateCost(awsProfile ?? undefined).catch(ignore("Cost Explorer isn't readable with this profile: no spend shown", () => null)),
    [awsProfile],
  );
  const { checking: checkingId } = useCliRead(awsCli, readIdentity, setAwsIdentity);
  useCliRead(awsCli, readCost, setMtdCost);
  return { profiles, awsIdentity, setAwsIdentity, checkingId, mtdCost };
}

/** The Azure subscriptions the signed-in account can see; none, with why, when they can't be
 *  read (a signed-out or missing `az` answers none; a failed read says why, as GCP's). */
async function readAzureSubscriptions(): Promise<{ subs: AzureSubscription[]; error: string | null }> {
  try {
    return { subs: await azureSubscriptions(), error: null };
  } catch (e: unknown) {
    warn("reading the Azure subscriptions")(e);
    return { subs: [], error: describeError(e) };
  }
}

/** Azure: the subscriptions the signed-in account can see, so you pick one instead of typing an
 *  id. The account's default subscription is chosen if none is yet. */
function useAzureSubscriptions(azure: boolean, setV: SetInput) {
  const [azureSubs, setAzureSubs] = useState<AzureSubscription[]>([]);
  const [azureError, setAzureError] = useState<string | null>(null);
  const apply = useCallback(
    ({ subs, error }: Awaited<ReturnType<typeof readAzureSubscriptions>>) => {
      setAzureSubs(subs);
      setAzureError(error);
      if (error) return;
      // Default to the account's default subscription if none chosen yet.
      setV((s) => (s.provider === "azure" && !s.username && subs.length > 0 ? { ...s, username: (subs.find((x) => x.isDefault) ?? subs[0]).id } : s));
    },
    [setV],
  );
  const { checking: azureChecking, reload: loadAzureSubs } = useCliRead(azure, readAzureSubscriptions, apply);
  return { azureSubs, azureChecking, azureError, loadAzureSubs };
}

/** GCP's signed-in account, billing accounts and organizations, each read on its own: one call
 *  failing (no permission to list organizations, say) keeps what the others found. */
const readGcpAccounts = () => Promise.allSettled([gcpAccount(), gcpBillingAccounts(), gcpOrganizations()]);

/** GCP: the signed-in account, its billing accounts and organizations. The first open billing
 *  account is chosen if none is yet. */
function useGcpAccounts(gcp: boolean, setV: SetInput) {
  const [gcpBilling, setGcpBilling] = useState<GcpBillingAccount[]>([]);
  const [gcpOrgs, setGcpOrgs] = useState<GcpOrganization[]>([]);
  const [gcpEmail, setGcpEmail] = useState<string | null>(null);
  const [gcpError, setGcpError] = useState<string | null>(null);
  const apply = useCallback(
    ([email, billing, orgs]: Awaited<ReturnType<typeof readGcpAccounts>>) => {
      setGcpEmail(email.status === "fulfilled" ? email.value : null);
      const accounts = billing.status === "fulfilled" ? billing.value : [];
      setGcpBilling(accounts);
      setGcpOrgs(orgs.status === "fulfilled" ? orgs.value : []);
      // What went wrong, of the reads that failed.
      const failed = [email, billing, orgs].find((r) => r.status === "rejected");
      setGcpError(failed ? describeError(failed.reason) : null);
      // Default to the first open billing account if none chosen yet.
      const open = accounts.find((b) => b.open) ?? accounts.at(0);
      setV((s) => (s.provider === "gcp" && !s.username && open ? { ...s, username: open.id } : s));
    },
    [setV],
  );
  const { checking: gcpChecking, reload: loadGcpProjects } = useCliRead(gcp, readGcpAccounts, apply);
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
      .catch(
        ignore("no OCI CLI config here: the form asks for its values", () => {
          setOciCfg(null);
        }),
      );
  }, [oci, editing, setV]);
  return ociCfg;
}
