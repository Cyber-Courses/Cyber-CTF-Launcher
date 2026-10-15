import { describe, expect, it } from "vitest";
import type { MachineMetrics, ProviderStatus, SystemReport } from "@/lib/tauri";
import {
  HISTORY,
  LOW_DISK_BYTES,
  appendUsage,
  dockerState,
  hypervisorDetail,
  isLowDisk,
  percent,
  setupCount,
  usage,
  versionNumber,
  vmSetup,
} from "@/features/machine/machine-status";
import { usableHypervisors } from "@/features/machine/hypervisors";

// Decimal, as the Machine page shows sizes.
const GB = 1e9;
const metrics = (patch: Partial<MachineMetrics> = {}): MachineMetrics => ({
  cpu: 12,
  memUsed: 4 * GB,
  memTotal: 16 * GB,
  diskUsed: 100 * GB,
  diskTotal: 500 * GB,
  uptimeSecs: 3600,
  cores: 8,
  containers: 2,
  ...patch,
});
const tool = (installed: boolean, version: string | null = null) => ({ installed, version });
const hv = (patch: Partial<ProviderStatus>): ProviderStatus => ({
  provider: "virtualbox",
  remote: false,
  available: false,
  hypervisor: true,
  plugin: null,
  pluginInstalled: false,
  reason: null,
  ...patch,
});
const report = (patch: Partial<SystemReport> = {}) =>
  ({
    os: "linux",
    arch: "x86_64",
    docker: tool(true, "Docker version 29.5.3, build d1c06ef"),
    dockerRunning: true,
    dockerDenied: false,
    vagrant: tool(true, "Vagrant 2.4.1"),
    vmProviders: [],
    ...patch,
  }) as SystemReport;

describe("usage", () => {
  it("turns used/total into percentages, 0 when the total is unknown", () => {
    expect(percent(1, 4)).toBe(25);
    expect(percent(1, 0)).toBe(0);
    expect(usage(metrics())).toEqual({ memPct: 25, diskPct: 20, memFree: 12 * GB, diskFree: 400 * GB });
  });

  it("flags a disk that is nearly full or short of room for a lab", () => {
    expect(isLowDisk(metrics())).toBe(false);
    expect(isLowDisk(metrics({ diskUsed: 460 * GB }))).toBe(true);
    expect(isLowDisk(metrics({ diskTotal: 100 * GB, diskUsed: 100 * GB - LOW_DISK_BYTES + 1 }))).toBe(true);
    // Sizes show in decimal GB: "21 GB free" is not low, "19 GB free" is.
    expect(isLowDisk(metrics({ diskTotal: 200 * GB, diskUsed: 179 * GB }))).toBe(false);
    expect(isLowDisk(metrics({ diskTotal: 200 * GB, diskUsed: 181 * GB }))).toBe(true);
    // An unknown disk (0 total) is not reported as full.
    expect(isLowDisk(metrics({ diskTotal: 0, diskUsed: 0 }))).toBe(false);
  });

  it("keeps the last points of each figure for the sparklines", () => {
    let h = { cpu: [] as number[], mem: [] as number[], disk: [] as number[] };
    for (let i = 0; i < HISTORY + 5; i++) h = appendUsage(h, metrics({ cpu: i }));
    expect(h.cpu).toHaveLength(HISTORY);
    expect(h.cpu.at(-1)).toBe(HISTORY + 4);
    expect(h.mem.at(-1)).toBe(25);
  });
});

describe("versionNumber", () => {
  it("keeps the number of a --version line", () => {
    expect(versionNumber(tool(true, "Docker version 29.5.3, build d1c06ef"))).toBe("29.5.3");
    expect(versionNumber(tool(true, "Vagrant 2.4"))).toBe("2.4");
    expect(versionNumber(tool(true, "unknown"))).toBeNull();
    expect(versionNumber(tool(false))).toBeNull();
  });
});

describe("dockerState", () => {
  it("is ready while the engine runs, unless the last test failed", () => {
    expect(dockerState(report(), null)).toBe("ready");
    expect(dockerState(report(), { result: "ok", at: 1 })).toBe("ready");
    expect(dockerState(report(), { result: "fail", at: 1 })).toBe("testFailed");
  });

  it("says why it isn't", () => {
    expect(dockerState(report({ dockerRunning: false, dockerDenied: true }), null)).toBe("denied");
    expect(dockerState(report({ dockerRunning: false }), null)).toBe("stopped");
    expect(dockerState(report({ dockerRunning: false, docker: tool(false) }), null)).toBe("missing");
  });
});

describe("vmSetup", () => {
  it("is not applicable without a usable hypervisor", () => {
    expect(vmSetup(report(), null, null).state).toBe("notApplicable");
  });

  it("runs on the preferred ready hypervisor, else the first", () => {
    const vmProviders = [hv({ provider: "virtualbox", available: true }), hv({ provider: "qemu", available: true })];
    const pick = vmSetup(report({ vmProviders }), "qemu", null);
    expect(pick.provider?.provider).toBe("qemu");
    expect(pick.automatic).toBe(false);
    const auto = vmSetup(report({ vmProviders }), null, { result: "fail", at: 1 });
    expect(auto.provider?.provider).toBe("virtualbox");
    expect(auto.automatic).toBe(true);
    expect(auto.state).toBe("testFailed");
  });

  it("explains a hypervisor that can't run once Vagrant is in place", () => {
    const vm = vmSetup(report({ vmProviders: [hv({ reason: "no /dev/kvm" })] }), null, null);
    expect(vm.state).toBe("blocked");
    expect(vm.blockedReason).toBe("No /dev/kvm.");
  });

  it("asks for Vagrant, or for a hypervisor", () => {
    expect(vmSetup(report({ vagrant: tool(false), vmProviders: [hv({ reason: "no /dev/kvm" })] }), null, null).state).toBe("vagrantMissing");
    expect(vmSetup(report({ vmProviders: [hv({ hypervisor: false })] }), null, null).state).toBe("needHypervisor");
  });

  it("counts what still needs setting up", () => {
    expect(setupCount("ready", "notApplicable")).toBe(0);
    expect(setupCount("testFailed", "ready")).toBe(0);
    expect(setupCount("stopped", "needHypervisor")).toBe(2);
    expect(setupCount("ready", "blocked")).toBe(1);
  });
});

describe("hypervisorDetail", () => {
  it("reads installed, plugin state, missing or built in", () => {
    expect(hypervisorDetail(hv({}))).toEqual({ kind: "installed" });
    expect(hypervisorDetail(hv({ plugin: "vagrant-qemu", pluginInstalled: true }))).toEqual({ kind: "installedWithPlugin", plugin: "vagrant-qemu" });
    expect(hypervisorDetail(hv({ plugin: "vagrant-qemu" }))).toEqual({ kind: "pluginMissing", plugin: "vagrant-qemu" });
    expect(hypervisorDetail(hv({ hypervisor: false }))).toEqual({ kind: "notInstalled" });
    expect(hypervisorDetail(hv({ hypervisor: null }))).toEqual({ kind: "builtIn" });
  });
});

describe("usage: edges", () => {
  it("is 0 % for an unknown or non-numeric total, and over 100 % when more is used than there is", () => {
    expect(percent(5, 0)).toBe(0);
    expect(percent(5, NaN)).toBe(0);
    expect(percent(150, 100)).toBe(150);
    expect(usage(metrics({ memUsed: 20 * GB, memTotal: 16 * GB })).memFree).toBe(-4 * GB);
  });

  it("flags low disk at exactly 90 % or just under the free-space floor, never with an unknown total", () => {
    expect(isLowDisk(metrics({ diskTotal: 1000 * GB, diskUsed: 900 * GB }))).toBe(true);
    expect(isLowDisk(metrics({ diskTotal: 1000 * GB, diskUsed: 899 * GB }))).toBe(false);
    expect(isLowDisk(metrics({ diskTotal: 100 * GB, diskUsed: 100 * GB - LOW_DISK_BYTES }))).toBe(false);
    expect(isLowDisk(metrics({ diskTotal: 100 * GB, diskUsed: 100 * GB - LOW_DISK_BYTES + 1 }))).toBe(true);
    expect(isLowDisk(metrics({ diskTotal: 0, diskUsed: 0 }))).toBe(false);
  });

  it("keeps only the newest HISTORY points, dropping the oldest", () => {
    const full = { cpu: Array.from({ length: HISTORY }, (_, i) => i), mem: [], disk: [] };
    const next = appendUsage(full, metrics({ cpu: 99 }));
    expect(next.cpu).toHaveLength(HISTORY);
    expect(next.cpu[0]).toBe(1);
    expect(next.cpu.at(-1)).toBe(99);
    expect(next.mem).toEqual([25]);
  });

  it("needs a dotted number for a version", () => {
    expect(versionNumber(tool(true, "Vagrant 2.4.1"))).toBe("2.4.1");
    expect(versionNumber(tool(true, "v1.2"))).toBe("1.2");
    expect(versionNumber(tool(true, "Docker version 29.5.3-rc1"))).toBe("29.5.3");
    expect(versionNumber(tool(true, "29"))).toBeNull();
    expect(versionNumber(tool(true, ""))).toBeNull();
    expect(versionNumber(tool(false))).toBeNull();
  });
});

describe("dockerState and vmSetup: edges", () => {
  it("denied comes before missing, and a failed test only matters while the engine runs", () => {
    expect(dockerState(report({ docker: tool(false), dockerRunning: false, dockerDenied: true }), null)).toBe("denied");
    expect(dockerState(report({ dockerRunning: false }), { result: "fail", at: 1 })).toBe("stopped");
    expect(dockerState(report(), { result: "fail", at: 1 })).toBe("testFailed");
  });

  it("a preference that is not ready yields the first ready hypervisor, and only a missing preference makes the pick automatic", () => {
    const two = report({ vmProviders: [hv({ provider: "virtualbox", available: true }), hv({ provider: "qemu", available: true })] });
    expect(vmSetup(two, "parallels", null)).toMatchObject({ provider: { provider: "virtualbox" }, automatic: false, state: "ready" });
    expect(vmSetup(two, null, null).automatic).toBe(true);
    expect(vmSetup(two, "qemu", null)).toMatchObject({ provider: { provider: "qemu" }, automatic: false });
    const one = report({ vmProviders: [hv({ provider: "qemu", available: true })] });
    expect(vmSetup(one, null, { result: "fail", at: 1 })).toMatchObject({ automatic: false, state: "testFailed" });
  });

  it("capitalises a blocked reason and ends it with a period, only once Vagrant is in place", () => {
    const blocked = hv({ provider: "libvirt", available: false, reason: "no /dev/kvm on this machine", plugin: "vagrant-libvirt", pluginInstalled: true });
    expect(vmSetup(report({ vmProviders: [blocked] }), null, null)).toMatchObject({ blockedReason: "No /dev/kvm on this machine.", state: "blocked" });
    expect(vmSetup(report({ vagrant: tool(false), vmProviders: [blocked] }), null, null)).toMatchObject({ blockedReason: null, state: "vagrantMissing" });
    // The plugin still missing: that is what setup fixes next, not a blocked hypervisor.
    expect(vmSetup(report({ vmProviders: [{ ...blocked, pluginInstalled: false }] }), null, null).state).toBe("vagrantMissing");
  });

  it("counts the lab types still to set up for every combination", () => {
    expect(setupCount("ready", "ready")).toBe(0);
    expect(setupCount("testFailed", "testFailed")).toBe(0);
    expect(setupCount("missing", "notApplicable")).toBe(1);
    expect(setupCount("ready", "needHypervisor")).toBe(1);
    expect(setupCount("denied", "blocked")).toBe(2);
    expect(setupCount("stopped", "vagrantMissing")).toBe(2);
  });
});

describe("usableHypervisors", () => {
  const all = [
    hv({ provider: "virtualbox", hypervisor: false }),
    hv({ provider: "utm", hypervisor: false }),
    hv({ provider: "vmware_esxi", remote: true, hypervisor: null }),
  ];
  const names = (r: SystemReport) => usableHypervisors(r).map((p) => p.provider);

  it("hides VirtualBox on Apple Silicon and UTM on an Intel Mac, keeps both on Linux, never a remote host", () => {
    expect(names(report({ os: "macos", arch: "aarch64", vmProviders: all }))).toEqual(["utm"]);
    expect(names(report({ os: "macos", arch: "arm64", vmProviders: all }))).toEqual(["utm"]);
    expect(names(report({ os: "macos", arch: "x86_64", vmProviders: all }))).toEqual(["virtualbox"]);
    expect(names(report({ os: "linux", arch: "aarch64", vmProviders: all }))).toEqual(["virtualbox", "utm"]);
  });

  it("always keeps a hypervisor that is already installed, wherever it is", () => {
    const installed = [hv({ provider: "virtualbox", hypervisor: true })];
    expect(names(report({ os: "macos", arch: "aarch64", vmProviders: installed }))).toEqual(["virtualbox"]);
  });
});
