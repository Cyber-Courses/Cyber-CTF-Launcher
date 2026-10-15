import { answer } from "@/lib/dev-mock";
import type { SystemReport } from "@/lib/tauri";

const missing = { installed: false, version: null };

/** A machine with Docker, Vagrant and VirtualBox ready (the dev mock's report). */
export const readyReport = (): SystemReport => structuredClone(answer("system_check", undefined) as SystemReport);

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
    dockerCompose: missing,
    vagrant: missing,
    terraform: missing,
    ovftool: missing,
    cloudClis: { aws: missing, azure: missing, gcloud: missing },
    vmProviders: r.vmProviders.map((p) => ({ ...p, available: false, hypervisor: p.remote ? null : false, pluginInstalled: false, reason: "Not installed" })),
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
    dockerDenied: true,
    dockerDeniedHint: {
      code: "dockerDenied",
      args: { user: "player" },
      message: "Docker is running, but your account can't use it yet (sudo usermod -aG docker player)",
    },
  };
}
