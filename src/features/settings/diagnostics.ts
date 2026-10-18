import type { AgentInfo, Provider, SystemReport } from "@/lib/tauri";

/** The settings the diagnostics report: the attack box image and VM, and the chosen hypervisor. */
export type DiagnosticsSettings = { attackImage: string; attackBox: string; vmProvider: Provider | null };

/**
 * The About section's "Copy diagnostics" text: the version, the system, the tools and the
 * settings that matter for a bug report. In English: it goes to support.
 */
export function diagnosticsText(version: string | null, report: SystemReport | null, agent: AgentInfo | null, settings: DiagnosticsSettings): string {
  const lines = [
    `Cyber CTF ${version ? `v${version}` : "(unknown version)"}`,
    report ? `OS: ${report.os} ${report.arch}` : null,
    report
      ? `Docker: ${report.dockerRunning ? `running (${report.dockerEngine ?? "unknown engine"})` : report.docker.installed ? "installed, not running" : "not installed"}`
      : null,
    report
      ? `Vagrant: ${report.vagrant.installed ? "installed" : "not installed"}, Terraform: ${report.terraform.installed ? "installed" : "not installed"}`
      : null,
    report
      ? `Hypervisors ready: ${
          report.vmProviders
            .filter((p) => !p.remote && p.available)
            .map((p) => p.provider)
            .join(", ") || "none"
        }`
      : null,
    `Attack box: ${settings.attackImage}`,
    `Attack VM: ${settings.attackBox}`,
    `VM provider: ${settings.vmProvider ?? "automatic"}`,
    agent ? `Install ID: ${agent.installId}` : "Not signed in",
  ].filter(Boolean);
  return lines.join("\n");
}
