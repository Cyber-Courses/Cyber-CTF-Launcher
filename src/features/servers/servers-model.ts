import type { HostCapacity, ServerHost, ServerTest, SystemReport } from "@/lib/tauri";
import type { Tone } from "@/components/ui/status-pill";
import { describeError, errorCode, type ErrorCode } from "@/lib/errors";

// The Servers page's logic, as pure functions of the hosts, their tests and capacity.

/** The hosts the Servers page lists: the hypervisors. Every cloud account (AWS, Azure, GCP,
 *  DigitalOcean, Linode, Oracle Cloud) lives on the Cloud page. */
export const onPremHosts = (hosts: ServerHost[]) => hosts.filter((h) => h.provider === "proxmox" || h.provider === "vmware_esxi");

/** Each host's last test by id ("testing" while one runs; none before the first). */
export type HostTests = Partial<Record<string, HostTest | "testing">>;

/** The hosts panel's tone: none yet, a test running, all online, some, or none. */
function summaryTone(count: number, online: number, anyTesting: boolean): Tone {
  if (count === 0) return "warn";
  if (anyTesting) return "muted";
  if (online === count) return "ok";
  return online > 0 ? "warn" : "fail";
}

/** The hosts panel's summary: how many answered the last test, and its tone. */
export function hostsSummary(hosts: ServerHost[], tests: HostTests) {
  const online = hosts.filter((h) => hostStatus(tests[h.id]).online).length;
  const anyTesting = hosts.some((h) => tests[h.id] === "testing");
  return { online, anyTesting, tone: summaryTone(hosts.length, online, anyTesting) };
}

/** The host the capacity panel shows: the default when it answered, else the first that did. */
export function capacityHost(hosts: ServerHost[], defaultId: string | null, caps: Partial<Record<string, HostCapacity | null>>) {
  const answered = hosts.flatMap((host) => {
    const cap = caps[host.id];
    return cap ? [{ host, cap }] : [];
  });
  return answered.find((a) => a.host.id === defaultId) ?? answered.at(0) ?? null;
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

/** What a host's last test says, as its status pill names it. */
type HostStatusKind = "testing" | "notTested" | "online" | "needsPassword" | "unreachable";

const STATUS_TONE: Record<HostStatusKind, Tone> = { testing: "muted", notTested: "muted", online: "ok", needsPassword: "warn", unreachable: "fail" };

function statusKind(test: HostTest | "testing" | undefined): HostStatusKind {
  if (test === "testing") return "testing";
  if (!test) return "notTested";
  if (test.ok) return "online";
  // A missing password is a credential to enter, not a host that is down.
  return test.errorCode === "noPasswordStored" ? "needsPassword" : "unreachable";
}

/** A host row's last test, read for its status pill: the result once it has one, whether it
 *  only lacks a stored password, the pill's tone and label, and whether the host answered. */
export function hostStatus(test: HostTest | "testing" | undefined) {
  const status = statusKind(test);
  const result = test === "testing" ? null : (test ?? null);
  return { result, needsPassword: status === "needsPassword", tone: STATUS_TONE[status], status, online: status === "online" } as const;
}

/** A host on a private LAN address: what macOS's Local Network permission governs. */
export const isPrivateHost = (h: string) => /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(h) || h.endsWith(".local");
