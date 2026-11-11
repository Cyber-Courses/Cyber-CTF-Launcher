// This machine in the browser preview: its system check (down until the test lets it come back,
// with `hold=check`), the one-click installs (an installer's output, then the tool reported
// installed; the first fails with `install=fail`), the self-tests and the setup window.
import type { DockerEngine, Need, SystemReport, UtilityState } from "@/lib/tauri";
import { argText, fail, send, sleep } from "@/lib/dev-mock/command";
import type { BrowserHandler, Live } from "@/lib/dev-mock/live";
import { scenarioReport } from "@/lib/dev-mock/scenario";
import { openWindow } from "@/lib/dev-mock/windows";
import { withSetup } from "@/lib/dev-mock/setup-options";

/** What Vagrant still needs to drive a hypervisor, as the core works it out (`providers::needs`). */
function vagrantNeeds(vagrant: boolean, plugin: boolean, utility: UtilityState | null): Need[] {
  return [
    ...(vagrant ? [] : (["vagrant"] as const)),
    ...(plugin ? [] : (["plugin"] as const)),
    ...(utility && utility !== "running" ? (["vmwareUtility"] as const) : []),
  ];
}

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
    const utility = p.utility && has("vmware-utility") ? "running" : p.utility;
    const needs = vagrantNeeds(vagrant.installed, pluginInstalled, utility);
    const available = hypervisor === true && needs.length === 0;
    return { ...p, hypervisor, pluginInstalled, utility, needs, available, reason: available ? null : p.reason };
  });
  return {
    ...r,
    pkgManager,
    docker: docker ? { installed: true, version: "27.3.1" } : r.docker,
    dockerRunning: docker || r.dockerRunning,
    dockerEngine: docker ? engine : r.dockerEngine,
    dockerEnginesRunning: docker ? [engine] : r.dockerEnginesRunning,
    dockerEngines: docker ? [{ engine, context: engine === "docker-engine" ? "default" : "desktop-linux", running: true, current: true }] : r.dockerEngines,
    dockerCompose: docker ? { installed: true, version: "2.29.7" } : r.dockerCompose,
    vagrant,
    terraform: has("terraform") ? { installed: true, version: "1.9.8" } : r.terraform,
    ...placedOvftool(r, has("ovftool")),
    vmProviders,
  };
}

/** Where the core places a downloaded OVF Tool, in the preview. */
const MANAGED_OVFTOOL = "/Users/player/Library/Application Support/org.cyberctf.desktop/tools/ovftool/ovftool";

/** The report's OVF Tool, once the preview placed one (`placed`): the app's copy, found. */
function placedOvftool(r: SystemReport, placed: boolean): Pick<SystemReport, "ovftool" | "ovftoolLocation"> {
  if (!placed) return { ovftool: r.ovftool, ovftoolLocation: r.ovftoolLocation };
  return {
    ovftool: { installed: true, version: "VMware ovftool 5.1.0 (build-24936374)" },
    ovftoolLocation: { source: "managed", path: MANAGED_OVFTOOL, needsRosetta: false },
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

/** Whether the machine is down now (`hold=check`, until the test lets it come back). */
const machineDown = (live: Live) => live.holds.isHeld("check");

// Decided when the answer is given: a check in flight when the machine comes back answers like
// the next one, so no failure lands after the test's retry.
const systemCheck: BrowserHandler = async (_args, { sc, live }) => {
  if (machineDown(live)) {
    await sleep(150);
    if (machineDown(live)) fail("docker info timed out after 20 s");
  }
  // `hold=pm`: missing until the test lets it show up.
  const pm = sc.held.includes("pm") ? !live.holds.isHeld("pm") : null;
  return withSetup(withReady(withEngine(withInstalls(scenarioReport(sc), live.installed, { pm }), live.engine)));
};

/** The report once Docker was pointed at `engine` ("Use by default"), when it runs: its context
 *  is the current one, and labs run on it. */
function withEngine(r: SystemReport, engine: DockerEngine | null): SystemReport {
  if (!engine || !r.dockerEngines.some((e) => e.engine === engine && e.running)) return r;
  return { ...r, dockerEngine: engine, dockerEngines: r.dockerEngines.map((e) => ({ ...e, current: e.engine === engine })) };
}

/** The report's ready hypervisors, as the core works them out (providers::ready): the local ones
 *  available, with Vagrant. */
function withReady(r: SystemReport): SystemReport {
  const ready = r.vagrant.installed ? r.vmProviders.filter((p) => !p.remote && p.available && p.hypervisor !== false).map((p) => p.provider) : [];
  return { ...r, readyHypervisors: ready };
}

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
  // The picked folder is taken as VMware's OVF Tool: placed, and reported by the next check.
  ovftool_place: async (_args, { live }) => {
    await sleep(400);
    live.installed.add("ovftool");
    return { outcome: "placed", path: MANAGED_OVFTOOL, needsRosetta: false };
  },
  machine_selftest: selftest,
  docker_use_engine: async (args, { live }) => {
    await sleep(150);
    live.engine = argText(args?.engine) as DockerEngine;
    return null;
  },
  machine_open_setup: (args) => {
    const step = args?.step ? `?step=${argText(args.step)}` : "";
    const provider = step && args?.provider ? `&provider=${argText(args.provider)}` : "";
    openWindow(`/machine-setup${step}${provider}`);
    return null;
  },
};
