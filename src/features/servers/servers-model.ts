import type { HostCapacity, ServerHost, ServerTest, SystemReport } from "@/lib/tauri";
import type { Tone } from "@/components/ui/status-pill";
import { describeError, errorCode, type ErrorCode } from "@/lib/errors";

// The Servers page's logic, as pure functions of the hosts, their tests and capacity.

/** The hosts the Servers page lists: the hypervisors. Every cloud account (AWS, Azure, GCP,
 *  DigitalOcean, Linode, Oracle Cloud) lives on the Cloud page. */
export const onPremHosts = (hosts: ServerHost[]) => hosts.filter((h) => h.provider === "proxmox" || h.provider === "vmware_esxi");

/** The hosts panel's summary: how many answered the last test, and its tone. */
export function hostsSummary(hosts: ServerHost[], tests: Record<string, ServerTest | "testing">) {
  const online = hosts.filter((h) => {
    const r = tests[h.id];
    return !!r && r !== "testing" && r.ok;
  }).length;
  const anyTesting = hosts.some((h) => tests[h.id] === "testing");
  const tone: Tone = hosts.length === 0 ? "warn" : anyTesting ? "muted" : online === hosts.length ? "ok" : online > 0 ? "warn" : "fail";
  return { online, anyTesting, tone };
}

/** The host the capacity panel shows: the default when it answered, else the first that did. */
export function capacityHost(hosts: ServerHost[], defaultId: string | null, caps: Record<string, HostCapacity | null>) {
  const host = hosts.find((h) => h.id === defaultId && caps[h.id]) ?? hosts.find((h) => caps[h.id]) ?? null;
  const cap = host ? caps[host.id] : null;
  return host && cap ? { host, cap } : null;
}

/** Whether this machine already runs VM labs itself (then a server isn't suggested). */
export const canRunVmHere = (report: SystemReport) => report.vmProviders.some((p) => !p.remote && p.available && p.hypervisor !== false);

/** A host's test: the backend's answer, or one that couldn't run (`errorCode` says why). */
export type HostTest = ServerTest & { errorCode?: ErrorCode | null };

/** A test that couldn't run, in the shape of one that failed (the row shows why). */
export const failedTest = (e: unknown): HostTest => ({
  ok: false,
  reachable: false,
  authenticated: null,
  latencyMs: null,
  message: describeError(e),
  checks: [],
  errorCode: errorCode(e),
});

/** A host row's last test, read for its status pill: the result once it has one, whether it
 *  only lacks a stored password, the pill's tone and label, and whether the host answered. */
export function hostStatus(test: HostTest | "testing" | undefined) {
  const result = test && test !== "testing" ? test : null;
  // A missing password is a credential to enter, not a host that is down.
  const needsPassword = !!result && !result.ok && result.errorCode === "noPasswordStored";
  const tone: Tone = !result ? "muted" : result.ok ? "ok" : needsPassword ? "warn" : "fail";
  const status = test === "testing" ? "testing" : !result ? "notTested" : result.ok ? "online" : needsPassword ? "needsPassword" : "unreachable";
  return { result, needsPassword, tone, status, online: !!result && result.ok } as const;
}

/** A host on a private LAN address: what macOS's Local Network permission governs. */
export const isPrivateHost = (h: string) => /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(h) || h.endsWith(".local");
