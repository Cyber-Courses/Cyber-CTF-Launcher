import type { CloudProvider, LabStatus, ServerHost, ServerTest } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";

// The Cloud page's logic, as pure functions of the saved accounts, their spend and the labs.

/** The providers offered up front: the first-run tiles, and a "connect" row each until connected. */
export const MAIN_PROVIDERS: { id: CloudProvider; label: string }[] = [
  { id: "aws", label: "Amazon Web Services" },
  { id: "azure", label: "Microsoft Azure" },
  { id: "gcp", label: "Google Cloud" },
];

const CLOUD_PROVIDERS: readonly string[] = ["aws", "azure", "gcp", "digitalocean", "linode", "oci"] satisfies CloudProvider[];

/** The cloud accounts among the saved hosts (the rest are servers). */
export const cloudAccounts = (hosts: ServerHost[]) => hosts.filter((h) => CLOUD_PROVIDERS.includes(h.provider));

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

/** Labs running on one of these accounts right now (a lab's status names the account it runs on). */
export function labsOnAccounts(labs: Lab[], statuses: Partial<Record<string, LabStatus>>, accounts: ServerHost[]) {
  const names = new Set(accounts.map((h) => h.name));
  return labs.filter((l) => {
    const s = statuses[l.id];
    return l.runtime && s?.running && !!s.host && names.has(s.host);
  });
}

/** How an account signs in, as its row names it. */
export type Credential = { kind: "apiToken" | "apiKey" | "cliSignIn" | "cliCredentials" | "accessKeys" } | { kind: "cliProfile"; profile: string };

export function credentialOf(h: ServerHost): Credential {
  if (h.provider === "digitalocean" || h.provider === "linode") return { kind: "apiToken" };
  if (h.provider === "oci") return { kind: "apiKey" };
  if (h.provider !== "aws") return { kind: "cliSignIn" };
  if (!h.useCliCreds) return { kind: "accessKeys" };
  return h.awsProfile ? { kind: "cliProfile", profile: h.awsProfile } : { kind: "cliCredentials" };
}

/** A test's outcome: failed on any failure, a warning when a check only warrants one, else ok. */
export type Outcome = "ok" | "warn" | "fail";

export function outcomeOf(result: ServerTest): Outcome {
  if (!result.ok) return "fail";
  return result.checks.some((c) => c.state === "warn") ? "warn" : "ok";
}
