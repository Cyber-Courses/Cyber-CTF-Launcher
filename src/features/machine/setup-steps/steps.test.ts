import { describe, expect, it, vi } from "vitest";
import type { SystemReport } from "@/lib/tauri";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";

// steps.ts pulls icons and the setup hook's module only for types; no Tauri runtime needed.
vi.mock("@/lib/tauri", () => ({}));

const { canContinue, nextLabel, nextVariant, stepMeta, wslState } = await import("@/features/machine/setup-steps/steps");

const report = (over: Partial<SystemReport> = {}) =>
  ({
    os: "linux",
    arch: "x86_64",
    docker: { installed: true },
    dockerRunning: true,
    dockerEngine: "docker-engine",
    pkgManager: { name: "apt", installed: true, version: "3.0" },
    vagrant: { installed: false },
    // Linux with nothing installed: VirtualBox and libvirt offered, neither set up.
    vmProviders: [
      { provider: "virtualbox", remote: false, hypervisor: false, available: false },
      { provider: "libvirt", remote: false, hypervisor: false, available: false },
    ],
    ...over,
  }) as unknown as SystemReport;
const setup = (over: Partial<{ engine: string | null; hypervisor: string | null }> = {}) =>
  ({ engine: null, hypervisor: null, ...over }) as unknown as MachineSetupState;

describe("machine setup steps", () => {
  it("a machine with no hypervisor can pass the VM steps, labelled as a skip", () => {
    // Regression: Continue stayed disabled on "Virtual machines" though the step calls a
    // hypervisor optional; the only way out was to abandon setup.
    expect(canContinue("vm", report(), setup())).toBe(true);
    expect(canContinue("vagrant", report(), setup())).toBe(true);
    expect(nextLabel("vm", report())).toBe("Skip for now");
    expect(nextLabel("vagrant", report())).toBe("Skip for now");
  });

  it("with a hypervisor set up, Vagrant is still required", () => {
    const r = report({
      vmProviders: [{ provider: "libvirt", remote: false, hypervisor: true, available: false, plugin: "vagrant-libvirt", pluginInstalled: false }],
    } as Partial<SystemReport>);
    expect(nextLabel("vm", r)).toBe("Continue");
    expect(canContinue("vagrant", r, setup())).toBe(false);
  });

  it("a working engine without a tile (Podman) passes the engine step unless another was picked", () => {
    const podman = report({ dockerEngine: "podman" } as Partial<SystemReport>);
    expect(canContinue("docker", podman, setup())).toBe(true);
    expect(canContinue("docker", podman, setup({ engine: "docker-engine" }))).toBe(false);
    expect(canContinue("docker", report({ dockerRunning: false }), setup())).toBe(false);
  });

  it("the Windows engine step doesn't claim WSL 2 was enabled (the Machine page's Fix opens it directly)", () => {
    const windows = stepMeta("docker", report({ os: "windows" } as Partial<SystemReport>)).description;
    expect(windows).toContain("WSL 2");
    expect(windows).not.toMatch(/you enabled/i);
    expect(stepMeta("docker", report()).description).not.toContain("WSL");
  });

  it("the WSL step needs no confirmation when the check found it on, and can always be passed", () => {
    const win = (state: string | null) =>
      report({ os: "windows", dockerRunning: false, wsl: state ? { state, reopensAfterRestart: true } : null } as Partial<SystemReport>);
    expect(nextLabel("virtualization", win("ready"))).toBe("Continue");
    expect(nextLabel("virtualization", win("needsReboot"))).toBe("I’ll restart later");
    expect(nextLabel("virtualization", win("missing"))).toBe("Skip for now");
    expect(nextLabel("virtualization", win("unknown"))).toBe("I’ve done this");
    expect(nextLabel("virtualization", win(null))).toBe("I’ve done this");
    for (const s of ["ready", "needsReboot", "missing", "unknown", null]) expect(canContinue("virtualization", win(s), setup())).toBe(true);
    // Turned on here: a restart is due, unless the check already finds it on.
    const turnedOn = { wslTurnedOn: true };
    expect(nextLabel("virtualization", win("missing"), undefined, turnedOn)).toBe("I’ll restart later");
    expect(wslState(win("unknown"), turnedOn)).toBe("needsReboot");
    expect(wslState(win("ready"), turnedOn)).toBe("ready");
    expect(wslState(null)).toBe("unknown");
  });

  it("keeps the jewel off a forward button that only moves past the step (one jewel per view)", () => {
    // No hypervisor: Skip for now is a way past, not the recommended action.
    for (const step of ["vm", "vagrant", "vm-test"] as const) expect(nextVariant(step, report(), setup()), step).toBe("secondary");
    // WSL: I've done this, I'll restart later and a skip beside Turn on WSL 2, the step's jewel.
    // Docker not running yet: a running engine would count WSL 2 as on (#224).
    const win = (state: string) => report({ os: "windows", dockerRunning: false, wsl: { state, reopensAfterRestart: true } } as Partial<SystemReport>);
    for (const state of ["unknown", "needsReboot", "missing"]) expect(nextVariant("virtualization", win(state), setup()), state).toBe("secondary");
    expect(nextVariant("virtualization", win("ready"), setup())).toBe("primary");
    // Continue while the step's own Install is still the thing to do: not the jewel either.
    const hypervisor = report({
      vmProviders: [{ provider: "libvirt", remote: false, hypervisor: true, available: false, plugin: "vagrant-libvirt", pluginInstalled: false }],
    } as Partial<SystemReport>);
    expect(nextVariant("vagrant", hypervisor, setup())).toBe("secondary");
    expect(nextVariant("docker", report({ dockerRunning: false }), setup())).toBe("secondary");
    // Set up: Continue is the next action.
    expect(nextVariant("docker", report(), setup())).toBe("primary");
    expect(nextVariant("vm", hypervisor, setup())).toBe("primary");
    // A hypervisor installed but not the one chosen ready: Continue passes, Install Vagrant is the jewel.
    const notReady = report({
      vmProviders: [
        { provider: "virtualbox", remote: false, hypervisor: true, available: false, plugin: null, pluginInstalled: true },
        { provider: "libvirt", remote: false, hypervisor: false, available: false, plugin: "vagrant-libvirt", pluginInstalled: false },
      ],
    } as Partial<SystemReport>);
    expect(canContinue("vagrant", notReady, setup({ hypervisor: "libvirt" }))).toBe(true);
    expect(nextVariant("vagrant", notReady, setup({ hypervisor: "libvirt" }))).toBe("secondary");
  });
});
