import { describe, expect, it, vi } from "vitest";
import type { SystemReport } from "@/lib/tauri";
import type { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";

// steps.ts pulls icons and the setup hook's module only for types; no Tauri runtime needed.
vi.mock("@/lib/tauri", () => ({}));

const { canContinue, nextLabel, stepMeta } = await import("@/features/machine/setup-steps/steps");

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
});
