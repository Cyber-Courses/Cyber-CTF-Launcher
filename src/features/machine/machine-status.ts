import type { MachineMetrics, ProviderStatus, SystemReport, Tool } from "@/lib/tauri";
import type { LastTest } from "@/lib/settings";
import { usableHypervisors } from "@/features/machine/hypervisors";
import type { LabKind } from "@/features/machine/machine-parts";
import { GB } from "@/lib/format";

// What the Machine page says about this machine, as pure functions of the system report, the
// live metrics and the last self-tests.

export const OS_NAME: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux" };

/** Free memory each lab type wants to start comfortably. */
export const WANT_FREE: Record<LabKind, number> = { docker: 2e9, vm: 8e9 };

/** Under this much free space the Machine page warns: a lab image plus the attack box needs more.
 *  In decimal GB, as the page shows sizes: "21 GB free" isn't low. */
export const LOW_DISK_BYTES = 20 * GB;

/** `used` as a percentage of `total` (0 when the total is unknown). */
export const percent = (used: number, total: number) => (total ? (used / total) * 100 : 0);

/** Memory and disk use as percentages, and what is left. */
export function usage(m: MachineMetrics) {
  return {
    memPct: percent(m.memUsed, m.memTotal),
    diskPct: percent(m.diskUsed, m.diskTotal),
    memFree: m.memTotal - m.memUsed,
    diskFree: m.diskTotal - m.diskUsed,
  };
}

/** Labs and the attack box are several GB each: warn before a download fails. */
export function isLowDisk(m: MachineMetrics) {
  const { diskPct, diskFree } = usage(m);
  return m.diskTotal > 0 && (diskPct >= 90 || diskFree < LOW_DISK_BYTES);
}

/** The version number in `docker --version`-style output (the tool's name stripped), if any. */
export const versionNumber = (tool: Tool) => tool.version?.match(/\d+\.\d+(\.\d+)?/)?.[0] ?? null;

/** Where container labs stand: ready (last test passed or none yet), test failed, or why not. */
export type DockerState = "ready" | "testFailed" | "denied" | "stopped" | "missing";

export function dockerState(report: SystemReport, last: LastTest | null): DockerState {
  if (report.docker.installed && report.dockerRunning) return last?.result === "fail" ? "testFailed" : "ready";
  if (report.dockerDenied) return "denied";
  return report.docker.installed ? "stopped" : "missing";
}

/** Where VM labs stand on this machine. */
export type VmState = "notApplicable" | "ready" | "testFailed" | "blocked" | "vagrantMissing" | "needHypervisor";

/** A hypervisor with Vagrant and its plugin in place that still can't run (no /dev/kvm, not in
 *  the kvm group): its own reason, since installing Vagrant again wouldn't help. */
function blockedReasonOf(report: SystemReport, hypervisors: ProviderStatus[]): string | null {
  if (!report.vagrant.installed) return null;
  const reason = hypervisors.find((p) => p.hypervisor === true && (!p.plugin || p.pluginInstalled) && !p.available && p.reason)?.reason;
  return reason ? `${reason.charAt(0).toUpperCase()}${reason.slice(1)}.` : null;
}

function vmStateOf({
  count,
  provider,
  blockedReason,
  hasHypervisor,
  last,
}: {
  count: number;
  provider: ProviderStatus | null;
  blockedReason: string | null;
  hasHypervisor: boolean;
  last: LastTest | null;
}): VmState {
  if (count === 0) return "notApplicable";
  if (provider) return last?.result === "fail" ? "testFailed" : "ready";
  if (blockedReason) return "blocked";
  return hasHypervisor ? "vagrantMissing" : "needHypervisor";
}

/** The VM side of this machine: the hypervisors it can use, the one labs run on, and its state. */
export function vmSetup(report: SystemReport, preferred: string | null, last: LastTest | null) {
  const hypervisors = usableHypervisors(report);
  const readyList: ProviderStatus[] = report.vmProviders.filter((p) => !p.remote && p.available && p.hypervisor !== false);
  const provider = readyList.find((p) => p.provider === preferred) ?? readyList.at(0) ?? null;
  const hasHypervisor = hypervisors.some((p) => p.hypervisor === true);
  const blockedReason = blockedReasonOf(report, hypervisors);
  const state = vmStateOf({ count: hypervisors.length, provider, blockedReason, hasHypervisor, last });
  // Several ready and no preference: the first one is picked automatically.
  const automatic = readyList.length > 1 && !preferred;
  return { hypervisors, provider, hasHypervisor, blockedReason, automatic, state };
}

/** How many lab types still need setting up (the header's "N to set up"). */
export const setupCount = (docker: DockerState, vm: VmState) =>
  (docker === "ready" || docker === "testFailed" ? 0 : 1) + (vm === "blocked" || vm === "vagrantMissing" || vm === "needHypervisor" ? 1 : 0);

/** A local hypervisor's line in Details: installed (with its plugin, or the plugin it lacks, or
 *  none needed), missing, or built in. */
export type HypervisorDetail = { kind: "installedWithPlugin" | "pluginMissing"; plugin: string } | { kind: "installed" | "notInstalled" | "builtIn" };

export function hypervisorDetail(p: ProviderStatus): HypervisorDetail {
  if (p.hypervisor === true) {
    if (!p.plugin) return { kind: "installed" };
    return { kind: p.pluginInstalled ? "installedWithPlugin" : "pluginMissing", plugin: p.plugin };
  }
  return { kind: p.hypervisor === false ? "notInstalled" : "builtIn" };
}

/** Points kept for a stat card's sparkline. */
export const HISTORY = 20;

/** The last `HISTORY` points of each usage figure, with this reading appended. */
export function appendUsage(h: UsageHistory, m: MachineMetrics): UsageHistory {
  const push = (a: number[], v: number) => [...a, v].slice(-HISTORY);
  const { memPct, diskPct } = usage(m);
  return { cpu: push(h.cpu, m.cpu), mem: push(h.mem, memPct), disk: push(h.disk, diskPct) };
}

export type UsageHistory = { cpu: number[]; mem: number[]; disk: number[] };
