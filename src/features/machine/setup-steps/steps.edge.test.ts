import { describe, expect, it, vi } from "vitest";
import type { SystemReport } from "@/lib/tauri";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import type { MessageKey, Vars } from "@/lib/i18n";

// steps.ts pulls icons and the setup hook's module only for types; no Tauri runtime needed.
vi.mock("@/lib/tauri", () => ({}));

const { canContinue, chosenEngine, chosenHypervisor, machineSteps, nextLabel, stepMeta } = await import("@/features/machine/setup-steps/steps");

type Hv = SystemReport["vmProviders"][number];
const hv = (p: Partial<Hv>): Hv => ({
  provider: "virtualbox",
  remote: false,
  available: false,
  hypervisor: false,
  plugin: null,
  pluginInstalled: false,
  reason: null,
  ...p,
});
const report = (over: Partial<SystemReport> = {}) =>
  ({
    os: "linux",
    arch: "x86_64",
    docker: { installed: true },
    dockerRunning: true,
    dockerEngine: "docker-engine",
    pkgManager: { name: "apt", installed: true, version: "3.0" },
    vagrant: { installed: true },
    vmProviders: [],
    ...over,
  }) as unknown as SystemReport;
const setup = (over: Partial<{ engine: string | null; hypervisor: string | null }> = {}) =>
  ({ engine: null, hypervisor: null, ...over }) as unknown as MachineSetupState;
// Labels as their keys, with the variables appended, so a test reads what was asked for.
const t = (key: MessageKey, vars?: Vars) => (vars ? `${key}:${JSON.stringify(vars)}` : key);
const ALL = machineSteps(report({ os: "windows" }));

describe("machine steps: per OS", () => {
  it("adds the virtualization step on Windows only, right after the package manager", () => {
    expect(machineSteps(report({ os: "windows" }))[1]).toBe("virtualization");
    for (const r of [null, report({ os: "macos" }), report({ os: "linux" })]) expect(machineSteps(r)).not.toContain("virtualization");
  });

  it("nothing can continue before the machine report is in", () => {
    for (const step of ALL) expect(canContinue(step, null, setup()), step).toBe(false);
  });

  it("the package manager step waits for it; the test and attack steps never block", () => {
    expect(canContinue("pkgmgr", report({ pkgManager: { name: "brew", installed: false, version: null } }), setup())).toBe(false);
    const bare = report({ docker: { installed: false, version: null }, dockerRunning: false, vagrant: { installed: false, version: null } });
    for (const step of ["attack", "docker-test", "vm-test", "virtualization", "vm"] as const) expect(canContinue(step, bare, setup()), step).toBe(true);
  });

  it("the engine step needs the engine in use to be the one picked", () => {
    expect(canContinue("docker", report(), setup({ engine: "colima" }))).toBe(false);
    expect(canContinue("docker", report(), setup({ engine: "docker-engine" }))).toBe(true);
    expect(canContinue("docker", report({ dockerRunning: false }), setup())).toBe(false);
  });

  it("the Vagrant step needs the plugin of the chosen hypervisor, and nothing without a hypervisor", () => {
    const vmware = hv({ provider: "vmware_desktop", hypervisor: true, plugin: "vagrant-vmware-desktop" });
    expect(canContinue("vagrant", report({ vmProviders: [vmware] }), setup())).toBe(false);
    expect(canContinue("vagrant", report({ vmProviders: [{ ...vmware, pluginInstalled: true }] }), setup())).toBe(true);
    expect(canContinue("vagrant", report({ vagrant: { installed: false, version: null }, vmProviders: [{ ...vmware, pluginInstalled: true }] }), setup())).toBe(
      false,
    );
    expect(canContinue("vagrant", report({ vagrant: { installed: false, version: null }, vmProviders: [hv({})] }), setup())).toBe(true);
  });
});

describe("step words and picks", () => {
  it("the Windows virtualization step says Done; VM steps say Skip without a hypervisor and Continue with one", () => {
    expect(nextLabel("virtualization", report({ dockerRunning: false }), t)).toBe("machine.steps.next.done");
    // Docker running proves WSL 2 works (#224): nothing to confirm.
    expect(nextLabel("virtualization", report({ dockerRunning: true }), t)).toBe("machine.steps.next.continue");
    for (const step of ["vm", "vagrant", "vm-test"] as const)
      expect(nextLabel(step, report({ vmProviders: [hv({})] }), t), step).toBe("machine.steps.next.skip");
    const ready = report({ vmProviders: [hv({ hypervisor: true })] });
    for (const step of ["vm", "vagrant", "vm-test"] as const) expect(nextLabel(step, ready, t), step).toBe("machine.steps.next.continue");
    expect(nextLabel("docker", report({ vmProviders: [hv({})] }), t)).toBe("machine.steps.next.continue");
  });

  it("names the package manager, adds the macOS heads-up only there, and the Windows engine text only there", () => {
    expect(stepMeta("pkgmgr", report({ pkgManager: { name: "brew", installed: false, version: null } }), t).description).toContain('"brew"');
    expect(stepMeta("pkgmgr", null, t).description).toContain("machine.steps.aPackageManager");
    expect(stepMeta("attack", report({ os: "macos" }), t).description).toBe("machine.steps.attack.description machine.steps.macTerminal");
    expect(stepMeta("attack", report({ os: "linux" }), t).description).toBe("machine.steps.attack.description");
    expect(stepMeta("docker", report({ os: "windows" }), t).description).toBe("machine.steps.docker.descriptionWindows");
    expect(stepMeta("vm-test", report({ os: "macos" }), t).description).toContain("machine.steps.macLocalNetwork");
  });

  it("the engine is the pick, else the one in use, else the OS's recommended one; a pick this OS lacks falls to its first engine", () => {
    expect(chosenEngine(report({ os: "windows", dockerEngine: null }), setup()).id).toBe("docker-desktop");
    expect(chosenEngine(report({ os: "linux", dockerEngine: null }), setup()).id).toBe("docker-engine");
    expect(chosenEngine(report({ os: "macos", dockerEngine: "colima" }), setup()).id).toBe("colima");
    expect(chosenEngine(report({ os: "linux", dockerEngine: "colima" }), setup({ engine: "orbstack" })).id).toBe("docker-desktop");
  });

  it("the hypervisor is an installed one that can run, else an installed one, else an installable one, else none", () => {
    const stuck = hv({ provider: "libvirt", hypervisor: true, available: false, reason: "no /dev/kvm", plugin: "vagrant-libvirt", pluginInstalled: true });
    const works = hv({ provider: "qemu", hypervisor: true, available: true });
    expect(chosenHypervisor(report({ vmProviders: [stuck, works] }), setup())?.provider).toBe("qemu");
    expect(chosenHypervisor(report({ vmProviders: [stuck] }), setup())?.provider).toBe("libvirt");
    expect(chosenHypervisor(report({ vmProviders: [hv({ provider: "vmware_desktop" }), hv({ provider: "virtualbox" })] }), setup())?.provider).toBe(
      "virtualbox",
    );
    expect(chosenHypervisor(report({ vmProviders: [] }), setup())).toBeUndefined();
    expect(chosenHypervisor(report({ vmProviders: [stuck, works] }), setup({ hypervisor: "libvirt" }))?.provider).toBe("libvirt");
  });
});
