import { describe, expect, it, vi } from "vitest";
import type { SystemReport } from "@/lib/tauri";

// Pure: no Tauri runtime needed (settings read localStorage inside a try, none here).
vi.mock("@/lib/tauri", () => ({}));

const { firstPending, pathProgress, stepDone } = await import("@/features/machine/setup-steps/step-status");
const { withNeeds } = await import("@/test/fixtures");

const ok = { result: "ok" as const, at: 1 };
const fail = { result: "fail" as const, at: 1 };
const none = { docker: null, vm: null };

/** A Linux machine with Docker running, VirtualBox and Vagrant in, Terraform in; each
 *  hypervisor's `needs` as the core lists them for its Vagrant and plugins. */
const ready = (over: Partial<SystemReport> = {}) =>
  withNeeds({
    os: "linux",
    arch: "x86_64",
    docker: { installed: true, version: "27.0" },
    dockerRunning: true,
    dockerEngine: "docker-engine",
    pkgManager: { name: "apt", installed: true, version: "3.0" },
    vagrant: { installed: true, version: "2.4.3" },
    terraform: { installed: true, version: "1.9.8" },
    vmProviders: [
      { provider: "virtualbox", remote: false, hypervisor: true, available: true, plugin: null, pluginInstalled: true, utility: null, reason: null },
    ],
    ...over,
  } as unknown as SystemReport);
/** Nothing installed yet. */
const fresh = (over: Partial<SystemReport> = {}) =>
  ready({
    docker: { installed: false, version: null },
    dockerRunning: false,
    dockerEngine: null,
    pkgManager: { name: "apt", installed: false, version: null },
    vagrant: { installed: false, version: null },
    terraform: { installed: false, version: null },
    vmProviders: [
      {
        provider: "virtualbox",
        remote: false,
        hypervisor: false,
        available: false,
        plugin: null,
        pluginInstalled: false,
        utility: null,
        needs: ["vagrant"],
        reason: null,
      },
    ],
    ...over,
  });

describe("where a path stands", () => {
  it("a machine with Docker installed and tested has nothing left on the container path", () => {
    expect(firstPending("docker", ready(), { ...none, docker: ok }, null)).toBeNull();
    expect(pathProgress("docker", ready(), { ...none, docker: ok }, null)).toEqual({ done: 4, total: 4 });
  });

  it("an untested engine leaves the test; a failed one too", () => {
    expect(firstPending("docker", ready(), none, null)).toBe("docker-test");
    expect(firstPending("docker", ready(), { ...none, docker: fail }, null)).toBe("docker-test");
    expect(firstPending("docker", ready({ dockerRunning: false }), { ...none, docker: ok }, null)).toBe("docker");
  });

  it("a fresh machine starts every path at the package manager, Windows' container path then needs WSL 2", () => {
    for (const path of ["docker", "vm"] as const) expect(firstPending(path, fresh(), none, null), path).toBe("pkgmgr");
    const windows = fresh({ os: "windows", pkgManager: { name: "winget", installed: true, version: "1.9" }, wsl: null });
    expect(firstPending("docker", windows, none, null)).toBe("virtualization");
    expect(pathProgress("docker", windows, none, null)).toEqual({ done: 2, total: 5 });
  });

  it("the VM path waits on the hypervisor, then Vagrant, then its test", () => {
    const noHypervisor = fresh({ pkgManager: { name: "apt", installed: true, version: "3.0" } });
    expect(firstPending("vm", noHypervisor, none, null)).toBe("vm");
    expect(firstPending("vm", ready({ vagrant: { installed: false, version: null } }), none, null)).toBe("vagrant");
    expect(firstPending("vm", ready(), none, null)).toBe("vm-test");
    expect(firstPending("vm", ready(), { ...none, vm: ok }, null)).toBeNull();
  });

  it("a hypervisor that can't run here, or one whose plugin is missing, leaves its step to do", () => {
    const stuck = {
      provider: "libvirt",
      remote: false,
      hypervisor: true,
      available: false,
      plugin: "vagrant-libvirt",
      pluginInstalled: true,
      reason: "no /dev/kvm",
    };
    expect(stepDone("vm", ready({ vmProviders: [stuck] } as Partial<SystemReport>), none, null)).toBe(false);
    const noPlugin = { ...stuck, available: true, pluginInstalled: false, reason: null };
    expect(stepDone("vm", ready({ vmProviders: [noPlugin] } as Partial<SystemReport>), none, null)).toBe(true);
    expect(stepDone("vagrant", ready({ vmProviders: [noPlugin] } as Partial<SystemReport>), none, null)).toBe(false);
    expect(stepDone("attack", fresh(), none, null)).toBe(true);
  });
});
