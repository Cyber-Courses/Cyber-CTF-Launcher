import type { CloudProvider, LabStatus, ServerHost } from "@/lib/tauri";
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

/** Whether an account set a monthly budget (null or 0 means no limit). */
export const hasBudget = (h: ServerHost) => !!h.monthlyLimit && h.monthlyLimit > 0;

/** Whether an account's month-to-date spend reached its budget. */
export const isOverBudget = (h: ServerHost, spent: number | null | undefined) => hasBudget(h) && spent != null && spent >= h.monthlyLimit!;

/** The accounts with a monthly budget (AWS only: Cost Explorer reads the spend). */
export const budgetedAccounts = (hosts: ServerHost[]) => hosts.filter((h) => h.provider === "aws" && hasBudget(h));

/** Labs running on one of these accounts right now (a lab's status names the account it runs on). */
export function labsOnAccounts(labs: Lab[], statuses: Record<string, LabStatus>, accounts: ServerHost[]) {
  const names = new Set(accounts.map((h) => h.name));
  return labs.filter((l) => {
    const s = statuses[l.id];
    return l.runtime && s?.running && !!s.host && names.has(s.host);
  });
}
