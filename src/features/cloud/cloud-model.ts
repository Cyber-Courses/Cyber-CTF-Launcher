import { HOST_KINDS, PROVIDER_NAMES, isCloudProvider, type CloudProvider, type LabStatus, type ServerHost } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";

// The Cloud page's logic, as pure functions of the saved accounts, their spend and the labs.

/** The providers offered up front: the first-run tiles, and a "connect" row each until connected. */
export const MAIN_PROVIDERS = (["aws", "azure", "gcp"] satisfies CloudProvider[]).map((id) => ({ id, label: PROVIDER_NAMES[id] }));

/** The main providers with no account among `accounts`, offered apart from them to connect. */
export const unconnectedProviders = (accounts: ServerHost[]) => MAIN_PROVIDERS.filter((p) => !accounts.some((h) => h.provider === p.id));

/** The cloud accounts among the saved hosts (the rest are servers). */
export const cloudAccounts = (hosts: ServerHost[]) => hosts.filter((h) => isCloudProvider(h.provider));

/** An account's monthly budget in USD, or null when it set none (null or 0 means no limit). */
export const budgetOf = (h: ServerHost): number | null => (h.monthlyLimit !== null && h.monthlyLimit > 0 ? h.monthlyLimit : null);

/** Whether an account's month-to-date spend reached its budget. */
export function isOverBudget(h: ServerHost, spent: number | null | undefined) {
  const limit = budgetOf(h);
  return limit !== null && spent != null && spent >= limit;
}

/** The accounts with a monthly budget, each with its limit (AWS only: Cost Explorer reads the spend). */
export function budgetedAccounts(hosts: ServerHost[]): { host: ServerHost; limit: number }[] {
  return hosts.flatMap((host) => {
    const limit = host.provider === "aws" ? budgetOf(host) : null;
    return limit === null ? [] : [{ host, limit }];
  });
}

/** Labs running on one of these accounts right now, by the id of the host the lab's status
 *  names (as `placeOf` reads it): a name can be shared, or changed since. */
export function labsOnAccounts(labs: Lab[], statuses: Partial<Record<string, LabStatus>>, accounts: ServerHost[]) {
  const ids = new Set(accounts.map((h) => h.id));
  return labs.filter((l) => {
    const s = statuses[l.id];
    return l.runtime && s?.running && s.hostId !== null && ids.has(s.hostId);
  });
}

/** How an account signs in, as its row names it. */
export type Credential = { kind: "apiToken" | "apiKey" | "cliSignIn" | "cliCredentials" | "accessKeys" } | { kind: "cliProfile"; profile: string };

/** How account `h` signs in: its kind's way, the backend's (`HOST_KINDS`); AWS's keys, or the AWS
 *  CLI's own credentials (a profile of it, or its default). */
export function credentialOf(h: ServerHost): Credential {
  const signIn = HOST_KINDS[h.provider].signIn;
  // A server's login never shows here: the Cloud page lists cloud accounts only.
  if (signIn !== "accessKeys") return { kind: signIn === "login" ? "cliSignIn" : signIn };
  if (!h.useCliCreds) return { kind: "accessKeys" };
  return h.awsProfile ? { kind: "cliProfile", profile: h.awsProfile } : { kind: "cliCredentials" };
}
