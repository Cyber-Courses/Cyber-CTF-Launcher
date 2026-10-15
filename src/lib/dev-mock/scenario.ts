// The browser preview's scenarios: the switches next to `?mock` in the URL, and the machine the
// system check reports for them.
import type { Provider, ProviderStatus, SystemReport } from "@/lib/tauri";
import { report } from "@/lib/dev-mock/sample";

/**
 * Switches for the browser preview, read from the URL next to `?mock` (the end-to-end tests use
 * them; `answer` (sample.ts) stays the plain sample data the component tests build on):
 *  - `os=macos|windows|linux`, `arch=arm64|x86_64`: the machine the system check reports.
 *  - `machine=fresh`: nothing installed yet (no package manager, engine, Vagrant or hypervisor).
 *  - `onboarding`: the first-run onboarding shows (on every load with the switch).
 *  - `auth=out`: signed out until "Sign in" is used.
 *  - `check=down`: every system check fails until the test brings the machine back
 *    (`window.__CYBERCTF_MOCK_MACHINE_UP__()`), so no background re-check can answer before the
 *    test has seen the failure.
 *  - `launch=fail`: the first lab start of each page load fails, after a few lines of output.
 *  - `pm=later`: the package manager is missing from the first round of checks of each page load,
 *    then there (as if the player ran its install command while the setup window was open).
 *  - `install=fail`: the first one-click install of each page load fails. Otherwise installs
 *    stream an installer's output and succeed: the next system check reports the tool installed.
 *  - `deploying=none`: no lab is deploying at load (GOAD Light otherwise is).
 *  - `cloud=none`: no cloud account saved (until one is saved in the setup).
 *  - `link=<url>`: the app was opened with this link (`cyberctf://labs/<slug>`).
 * Labs started or stopped here stay so until a reload: Docker labs stream a Compose log, VM labs
 * a Vagrant log (on this machine's hypervisor or a saved server; pause, shut down, resume and
 * re-run provisioning work), cloud labs a Terraform log (outputs and an auto-stop), hosted labs
 * go from requested to running on the next poll. The attack box shell is a stand-in that answers
 * `id`, `whoami`, `hostname`, `ip -br a`, and ends on `exit`. Events emitted in one tab (Settings asking
 * the main window to show the onboarding again, a server saved in the setup) reach the others.
 */
export type Scenario = {
  os: "macos" | "windows" | "linux";
  arch: "arm64" | "x86_64";
  fresh: boolean;
  onboarding: boolean;
  signedOut: boolean;
  checkDown: boolean;
  launchFails: boolean;
  pmLater: boolean;
  installFails: boolean;
  noDeploying: boolean;
  noCloud: boolean;
  link: string | null;
};

export function scenarioFrom(search: string): Scenario {
  const q = new URLSearchParams(search);
  const os = q.get("os");
  return {
    os: os === "windows" || os === "linux" ? os : "macos",
    arch: q.get("arch") === "x86_64" ? "x86_64" : "arm64",
    fresh: q.get("machine") === "fresh",
    onboarding: q.has("onboarding"),
    signedOut: q.get("auth") === "out",
    checkDown: q.get("check") === "down",
    launchFails: q.get("launch") === "fail",
    pmLater: q.get("pm") === "later",
    installFails: q.get("install") === "fail",
    noDeploying: q.get("deploying") === "none",
    noCloud: q.get("cloud") === "none",
    link: q.get("link"),
  };
}

const notInstalled = { installed: false, version: null };

/** The local hypervisors each OS reports (the remote ones are the same everywhere): none
 *  installed on a fresh machine. */
function localProviders({ os, fresh }: Pick<Scenario, "os" | "fresh">) {
  const p = (provider: Provider, plugin: string | null = null): ProviderStatus => ({
    provider,
    remote: false,
    available: !fresh && provider === "virtualbox",
    hypervisor: fresh ? false : provider === "virtualbox",
    plugin,
    pluginInstalled: !plugin,
    reason: !fresh && provider === "virtualbox" ? null : "Not installed",
  });
  if (os === "windows") return [p("virtualbox"), p("vmware_desktop", "vagrant-vmware-desktop"), { ...p("hyperv"), hypervisor: null }];
  if (os === "linux") return [p("virtualbox"), p("libvirt", "vagrant-libvirt"), p("qemu", "vagrant-qemu")];
  return [p("virtualbox"), p("vmware_desktop", "vagrant-vmware-desktop"), p("utm", "vagrant_utm"), p("parallels", "vagrant-parallels")];
}

const PKG_MANAGER: Record<Scenario["os"], string> = { windows: "winget", linux: "apt", macos: "brew" };

/** On an Apple Silicon Mac VirtualBox can't run, so a ready machine there has UTM instead. */
function onAppleSilicon(p: ProviderStatus): ProviderStatus {
  if (p.provider === "virtualbox") return { ...p, available: false, hypervisor: false, reason: "VirtualBox does not run on Apple Silicon" };
  if (p.provider === "utm") return { ...p, available: true, hypervisor: true, pluginInstalled: true, reason: null };
  return p;
}

/** `ready` with nothing installed yet: no package manager, engine, Vagrant, Terraform, cloud CLI
 *  or remote provider plugin. */
function fresh(ready: SystemReport): SystemReport {
  return {
    ...ready,
    pkgManager: { ...ready.pkgManager, installed: false, version: null },
    docker: notInstalled,
    dockerRunning: false,
    dockerEngine: null,
    dockerEnginesRunning: [],
    dockerCompose: notInstalled,
    vagrant: notInstalled,
    terraform: notInstalled,
    cloudClis: { aws: notInstalled, azure: notInstalled, gcloud: notInstalled },
    vmProviders: ready.vmProviders.map((p) => (p.remote ? { ...p, available: false, pluginInstalled: false } : p)),
  };
}

/** The system check's answer for a scenario. */
export function scenarioReport(sc: Scenario): SystemReport {
  const appleSilicon = !sc.fresh && sc.os === "macos" && sc.arch === "arm64";
  const local = localProviders(sc).map((p) => (appleSilicon ? onAppleSilicon(p) : p));
  const engine = sc.os === "linux" ? "docker-engine" : "docker-desktop";
  const ready: SystemReport = {
    ...report,
    os: sc.os,
    arch: sc.arch,
    pkgManager: { name: PKG_MANAGER[sc.os], installed: true, version: "1.9.0" },
    dockerRunning: true,
    dockerDenied: false,
    dockerDeniedHint: null,
    dockerEngine: engine,
    dockerEnginesRunning: [engine],
    vmProviders: [...local, ...report.vmProviders.filter((p) => p.remote)],
  };
  return sc.fresh ? fresh(ready) : ready;
}
