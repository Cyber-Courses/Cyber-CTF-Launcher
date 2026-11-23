import type { AgentInfo, Provider, SystemReport } from "@/lib/tauri";
import { getAttackDesktop, getAttackImage, getAutoAttackBox, getDockerEngine, getLocalVmForm, getPortMode, getVmProvider } from "@/lib/settings";

/** The settings the diagnostics report: the attack box image and VM, and the chosen hypervisor. */
export type DiagnosticsSettings = { attackImage: string; vmProvider: Provider | null };

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
    report ? `Hypervisors ready: ${report.readyHypervisors.join(", ") || "none"}` : null,
    `Attack box: ${settings.attackImage}`,
    `VM provider: ${settings.vmProvider ?? "automatic"}`,
    agent ? `Install ID: ${agent.installId}` : "Not signed in",
  ].filter(Boolean);
  return lines.join("\n");
}

/**
 * The settings this window keeps that a diagnostics bundle carries (Rust adds the policy, the
 * update channel and the saved servers' kinds): what the launch and the attack box run with, and
 * the install's id when signed in. Nothing secret is kept among them.
 */
export function bundleSettings(agent: AgentInfo | null): Record<string, string> {
  const settings: Record<string, string> = {
    attackImage: getAttackImage(),
    attackBoxAuto: String(getAutoAttackBox()),
    attackDesktop: String(getAttackDesktop()),
    vmProvider: getVmProvider() ?? "automatic",
    dockerEngine: getDockerEngine() ?? "automatic",
    portMode: getPortMode() ?? "automatic",
    localVmForm: getLocalVmForm() ?? "automatic",
  };
  if (agent) settings.installId = agent.installId;
  return settings;
}

/** The bundle's suggested file name at `now`: `cyberctf-diagnostics-<yyyymmdd>-<hhmm>.tar.gz`. */
export function bundleFileName(now: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `cyberctf-diagnostics-${String(now.getFullYear())}${two(now.getMonth() + 1)}${two(now.getDate())}-${two(now.getHours())}${two(now.getMinutes())}.tar.gz`;
}

/** The launcher's support address, where error reports go. */
export const SUPPORT_EMAIL = "support@cyberctf.com";

/** A `mailto:` link to support with `subject` and `body`. A mail link can't attach a file: the
 *  body tells the player to attach the bundle. */
export function supportMail(subject: string, body: string): string {
  return `mailto:${SUPPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
}
