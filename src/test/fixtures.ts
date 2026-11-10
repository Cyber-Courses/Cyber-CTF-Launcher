import { answer } from "@/lib/dev-mock";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";

const missing = { installed: false, version: null };

/** A machine with Docker, Vagrant and VirtualBox ready (the dev mock's report). */
export const readyReport = (): SystemReport => structuredClone(answer("system_check", undefined) as SystemReport);

/** A report's hypervisors, `vmProviders` and the `readyHypervisors` the backend sends with them
 *  (Rust's `providers::ready`, Vagrant installed): for a test that builds its own report. */
export const hypervisors = (vmProviders: ProviderStatus[]): Pick<SystemReport, "readyHypervisors" | "vmProviders"> => ({
  vmProviders,
  readyHypervisors: vmProviders.filter((p) => !p.remote && p.available && p.hypervisor !== false).map((p) => p.provider),
});

/** No local hypervisor installed: none ready, each reported missing. */
const noHypervisors = (r: SystemReport): Pick<SystemReport, "readyHypervisors" | "vmProviders"> => ({
  readyHypervisors: [],
  vmProviders: r.vmProviders.map((p) => ({ ...p, available: false, hypervisor: p.remote ? null : false, pluginInstalled: false, reason: "Not installed" })),
});

/** Docker ready, but no local hypervisor: a container lab starts here without the "Where should it
 *  run?" choice (no VM-here option, which any ready hypervisor would offer). */
export function dockerOnlyReport(): SystemReport {
  const r = readyReport();
  return {
    ...r,
    ...noHypervisors(r),
  };
}

/** A fresh machine: nothing installed yet. */
export function freshReport(os = "macos"): SystemReport {
  const r = readyReport();
  return {
    ...r,
    os,
    arch: "x86_64",
    pkgManager: { name: os === "windows" ? "winget" : os === "linux" ? "apt" : "brew", installed: false, version: null },
    docker: missing,
    dockerRunning: false,
    dockerEngine: null,
    dockerEnginesRunning: [],
    dockerEngines: [],
    dockerCompose: missing,
    vagrant: missing,
    terraform: missing,
    ovftool: missing,
    cloudClis: { aws: missing, azure: missing, gcloud: missing },
    ...noHypervisors(r),
  };
}

/** Docker installed but its engine stopped; Linux user not in the docker group. */
export function stoppedDockerReport(): SystemReport {
  return {
    ...readyReport(),
    os: "linux",
    dockerRunning: false,
    dockerEngine: null,
    dockerEnginesRunning: [],
    // The native daemon's context, which refuses this account.
    dockerEngines: [{ engine: "docker-engine", context: "default", running: false, current: true }],
    dockerDenied: true,
    dockerDeniedHint: {
      code: "dockerDenied",
      args: { user: "player" },
      message: "Docker is running, but your account can't use it yet (sudo usermod -aG docker player)",
    },
  };
}

/** A local hypervisor as the machine check reports it: VirtualBox, installed and able to run labs,
 *  with `over`. */
export const hypervisorStatus = (over: Partial<ProviderStatus>): ProviderStatus => ({
  provider: "virtualbox",
  remote: false,
  available: true,
  hypervisor: true,
  plugin: null,
  pluginInstalled: true,
  reason: null,
  ...over,
});

/** The `managed_settings` answer of a computer whose policy turns one-click tool installs off. */
export const INSTALLS_OFF = { managed_settings: () => ({ policy: ["DisableToolInstalls"], updates: "inApp" }) };

/** What the UI says beside the install buttons that policy turns off. */
export const INSTALLS_OFF_NOTE = "Your organization turned off one-click tool installs and removals";
