import { describe, expect, it } from "vitest";

import { emulatorReady, hostSupports, localProviders, pickLocalHypervisor, readyHypervisors, runPlaces, runsNatively } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { Provider, ServerHost, SystemReport } from "@/lib/tauri";

const rt = (p: Partial<LabRuntimeInfo>): LabRuntimeInfo => ({ runtime: "VM", architectures: [], providers: [], ...p });
const vmProvider = (provider: string, p: { remote?: boolean; available?: boolean; hypervisor?: boolean | null } = {}) => ({
  provider: provider as Provider,
  remote: false,
  available: true,
  hypervisor: true,
  ...p,
});
const host = (provider: string) => ({ id: provider, name: provider, provider }) as unknown as ServerHost;

describe("runsNatively", () => {
  it("runs anywhere when the lab lists no CPU", () => {
    expect(runsNatively(rt({}), "arm64")).toBe(true);
  });
  it("runs only on the CPUs the lab lists", () => {
    expect(runsNatively(rt({ architectures: ["x86_64"] }), "x86_64")).toBe(true);
    expect(runsNatively(rt({ architectures: ["x86_64"] }), "arm64")).toBe(false);
  });
});

describe("readyHypervisors", () => {
  const report = {
    vagrant: { installed: true },
    vmProviders: [
      vmProvider("virtualbox"),
      vmProvider("qemu"),
      vmProvider("parallels", { available: false }),
      vmProvider("vmware_desktop", { hypervisor: false }),
      vmProvider("vmware_esxi", { remote: true }),
    ],
  };
  it("keeps the local, available hypervisors", () => {
    expect(readyHypervisors(report, null)).toEqual(["virtualbox", "qemu"]);
  });
  it("puts the preferred one first when it is ready", () => {
    expect(readyHypervisors(report, "qemu")).toEqual(["qemu", "virtualbox"]);
    expect(readyHypervisors(report, "parallels")).toEqual(["virtualbox", "qemu"]);
  });
  it("is empty without Vagrant", () => {
    expect(readyHypervisors({ ...report, vagrant: { installed: false } }, null)).toEqual([]);
  });
});

describe("emulatorReady", () => {
  it("needs Vagrant and a local QEMU", () => {
    const report = (installed: boolean, providers: ReturnType<typeof vmProvider>[]) =>
      ({ vagrant: { installed }, vmProviders: providers }) as unknown as SystemReport;
    expect(emulatorReady(report(true, [vmProvider("qemu")]))).toBe(true);
    expect(emulatorReady(report(false, [vmProvider("qemu")]))).toBe(false);
    expect(emulatorReady(report(true, [vmProvider("qemu", { remote: true })]))).toBe(false);
    expect(emulatorReady(report(true, [vmProvider("virtualbox")]))).toBe(false);
    expect(emulatorReady(null)).toBe(false);
  });
});

describe("localProviders", () => {
  const lab = rt({ architectures: ["x86_64"], providers: ["virtualbox", "qemu", "vmware_esxi", "aws", "hosted"] as Provider[] });
  it("leaves servers, clouds and hosted out", () => {
    expect(localProviders(lab, "x86_64")).toEqual(["virtualbox", "qemu"]);
  });
  it("is the emulator alone for a VM lab built for another CPU", () => {
    expect(localProviders(lab, "arm64")).toEqual(["qemu"]);
  });
});

describe("pickLocalHypervisor", () => {
  const local = ["parallels", "virtualbox"] as Provider[];
  it("takes a ready hypervisor the lab can use, never just the first listed", () => {
    expect(pickLocalHypervisor(local, ["virtualbox"] as Provider[], null)).toBe("virtualbox");
    expect(pickLocalHypervisor(local, ["qemu"] as Provider[], null)).toBeNull();
  });
  it("falls back to the preference, then the first one, before the report is in", () => {
    expect(pickLocalHypervisor(local, undefined, "virtualbox")).toBe("virtualbox");
    expect(pickLocalHypervisor(local, undefined, "qemu")).toBe("parallels");
    expect(pickLocalHypervisor([], undefined, null)).toBeNull();
  });
});

describe("hostSupports", () => {
  it("needs the lab to list the host's provider", () => {
    expect(hostSupports(rt({ runtime: "DOCKER", providers: ["aws"] as Provider[] }), host("aws"))).toBe(true);
    expect(hostSupports(rt({ runtime: "DOCKER", providers: ["aws"] as Provider[] }), host("gcp"))).toBe(false);
    expect(hostSupports(null, host("aws"))).toBe(false);
  });
  it("keeps VM labs off the clouds that can't run them yet", () => {
    expect(hostSupports(rt({ providers: ["aws", "azure"] as Provider[] }), host("aws"))).toBe(true);
    expect(hostSupports(rt({ providers: ["aws", "azure"] as Provider[] }), host("azure"))).toBe(false);
  });
});

describe("runPlaces", () => {
  const available = (places: ReturnType<typeof runPlaces>) => places.filter((p) => p.available).map((p) => p.key);
  it("lists a container lab's places", () => {
    expect(available(runPlaces(rt({ runtime: "DOCKER", providers: ["docker", "aws"] as Provider[], hosted: true })))).toEqual([
      "container",
      "local_vm",
      "cloud",
      "hosted",
    ]);
  });
  it("offers a foreign-CPU VM lab here only with an emulator", () => {
    const lab = rt({ architectures: ["x86_64"], providers: ["virtualbox", "proxmox"] as Provider[] });
    expect(available(runPlaces(lab, "arm64"))).toEqual(["server"]);
    expect(available(runPlaces(lab, "arm64", true))).toEqual(["local_vm", "server"]);
  });
});
