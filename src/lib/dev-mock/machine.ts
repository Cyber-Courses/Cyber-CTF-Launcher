// This machine in the browser preview: its system check (down until the test brings it back with
// `check=down`), the one-click installs (an installer's output, then the tool reported installed;
// the first fails with `install=fail`), the self-tests and the setup window.
import type { SystemReport } from "@/lib/tauri";
import { argText, fail, send, sleep } from "@/lib/dev-mock/command";
import type { BrowserHandler, Live } from "@/lib/dev-mock/live";
import { scenarioReport } from "@/lib/dev-mock/scenario";
import { openWindow } from "@/lib/dev-mock/windows";

/** The system check once the preview's one-click installs ran (`installed`: dependencies and
 *  Vagrant plugins), with the package manager there or not as `pm` says (null: as reported). */
function withInstalls(r: SystemReport, installed: ReadonlySet<string>, { pm }: { pm: boolean | null }): SystemReport {
  const pkgManager = pm === null ? r.pkgManager : { ...r.pkgManager, installed: pm, version: pm ? "4.4.0" : null };
  if (!installed.size) return { ...r, pkgManager };
  const has = (tool: string) => installed.has(tool);
  const engine = r.os === "linux" ? "docker-engine" : "docker-desktop";
  const docker = has("docker");
  const vagrant = has("vagrant") ? { installed: true, version: "2.4.3" } : r.vagrant;
  const vmProviders = r.vmProviders.map((p) => {
    if (p.remote) return p;
    const hypervisor = has(p.provider) || p.hypervisor;
    const pluginInstalled = p.pluginInstalled || (!!p.plugin && has(p.plugin));
    const available = hypervisor === true && vagrant.installed && pluginInstalled;
    return { ...p, hypervisor, pluginInstalled, available, reason: available ? null : p.reason };
  });
  return {
    ...r,
    pkgManager,
    docker: docker ? { installed: true, version: "27.3.1" } : r.docker,
    dockerRunning: docker || r.dockerRunning,
    dockerEngine: docker ? engine : r.dockerEngine,
    dockerEnginesRunning: docker ? [engine] : r.dockerEnginesRunning,
    dockerCompose: docker ? { installed: true, version: "2.29.7" } : r.dockerCompose,
    vagrant,
    terraform: has("terraform") ? { installed: true, version: "1.9.8" } : r.terraform,
    vmProviders,
  };
}

/** What each one-click install prints, as the core streams it. */
function installLines(what: string): string[] {
  if (what.startsWith("vagrant-") || what.startsWith("vagrant_"))
    return [`Installing the '${what}' plugin. This can take a few minutes...`, `Installed the plugin '${what} (0.3.1)'!`];
  return [`==> Downloading ${what}`, `==> Installing ${what}`, `🍺  ${what} was successfully installed!`];
}

const SELFTEST_STEPS: Partial<Record<string, string[]>> = {
  docker: ["engine", "pull", "start", "network", "port", "cleanup"],
  vm: ["vagrant", "provider", "hypervisor", "box", "boot", "exec", "network", "cleanup"],
};

/** Whether the machine is down now (`check=down`, until the test brings it back). */
const machineDown = (live: Live) => live.checkDown;

// Decided when the answer is given: a check in flight when the machine comes back answers like
// the next one, so no failure lands after the test's retry.
const systemCheck: BrowserHandler = async (_args, { sc, live }) => {
  if (machineDown(live)) {
    await sleep(150);
    if (machineDown(live)) fail("docker info timed out after 20 s");
  }
  const now = Date.now();
  if (!live.firstCheckAt) live.firstCheckAt = now;
  return withInstalls(scenarioReport(sc), live.installed, { pm: sc.pmLater ? now - live.firstCheckAt >= 1000 : null });
};

/** A one-click install of `what` (a dependency or a Vagrant plugin). */
const install =
  (key: "dependency" | "plugin"): BrowserHandler =>
  async (args, { live }) => {
    const what = argText(args?.[key]);
    const lines = installLines(what);
    for (const line of lines.slice(0, -1)) {
      send(args, line);
      await sleep(150);
    }
    if (live.installFails) {
      live.installFails = false;
      fail(`${what}: the installer exited with code 1 (dev mock)`);
    }
    send(args, lines.at(-1));
    live.installed.add(what);
    return null;
  };

const selftest: BrowserHandler = async (args) => {
  for (const step of SELFTEST_STEPS[argText(args?.kind)] ?? []) {
    send(args, { step, state: "running", detail: null }, "events");
    await sleep(60);
    send(args, { step, state: "ok", detail: null }, "events");
  }
  return null;
};

/** The machine commands of the preview. */
export const machineHandlers: Record<string, BrowserHandler> = {
  system_check: systemCheck,
  hyperv_join_admins: async () => {
    await sleep(300);
    return null;
  },
  install_dependency: install("dependency"),
  install_vagrant_plugin: install("plugin"),
  machine_selftest: selftest,
  machine_open_setup: (args) => {
    const step = args?.step ? `?step=${argText(args.step)}` : "";
    openWindow(`/machine-setup${step}`);
    return null;
  },
};
