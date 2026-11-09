import { describe, expect, it } from "vitest";

import { emulatorReady, formProviders, hostSupports, localProviders, preferredFirst, runPlaces, runsNatively } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabRunOptions, LabRunPath, Provider, ServerHost } from "@/lib/tauri";

const rt = (p: Partial<LabRuntimeInfo>): LabRuntimeInfo => ({ runtime: "VM", architectures: [], providers: [], ...p });
const host = (provider: string) => ({ id: provider, name: provider, provider }) as unknown as ServerHost;
const path = (target: string, provider: Provider): LabRunPath => ({ provider, target, variant: provider, badge: "none", evidence: "" });
/** The backend's run options with only `paths` set. */
const options = (paths: Partial<LabRunOptions>): LabRunOptions => ({
  docker: null,
  dockerVm: [],
  vms: [],
  servers: [],
  cloudDocker: [],
  cloudVm: [],
  cloudServices: [],
  refused: [],
  ...paths,
});

describe("runsNatively", () => {
  it("runs anywhere when the lab lists no CPU", () => {
    expect(runsNatively(rt({}), "arm64")).toBe(true);
  });
  it("runs only on the CPUs the lab lists", () => {
    expect(runsNatively(rt({ architectures: ["x86_64"] }), "x86_64")).toBe(true);
    expect(runsNatively(rt({ architectures: ["x86_64"] }), "arm64")).toBe(false);
  });
});

describe("preferredFirst", () => {
  const ready = ["virtualbox", "qemu"] as Provider[];
  it("puts the preferred one first when it is ready", () => {
    expect(preferredFirst(ready, "qemu")).toEqual(["qemu", "virtualbox"]);
    expect(preferredFirst(ready, "parallels")).toEqual(["virtualbox", "qemu"]);
    expect(preferredFirst(ready, null)).toEqual(["virtualbox", "qemu"]);
  });
});

describe("emulatorReady", () => {
  it("is QEMU among the hypervisors ready here", () => {
    expect(emulatorReady(["virtualbox", "qemu"])).toBe(true);
    expect(emulatorReady(["virtualbox"])).toBe(false);
    expect(emulatorReady([])).toBe(false);
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

describe("formProviders", () => {
  it("are the hypervisors the backend's run options give the form, the emulator alone for another CPU's VMs", () => {
    // Regression: the picker kept its own tables (every local hypervisor but UTM, any cloud):
    // what a lab runs on is what Isoloom generates it for, which the backend says.
    const vms = ["virtualbox", "hyperv", "qemu"].map((p) => path("vagrant", p as Provider));
    const both = rt({ runtime: "DOCKER", docker: true, vm: true, architectures: ["x86_64"] });
    expect(formProviders(vms, both)).toEqual(["virtualbox", "hyperv", "qemu"]);
    expect(formProviders(vms, both, "x86_64")).toEqual(["virtualbox", "hyperv", "qemu"]);
    expect(formProviders(vms, both, "aarch64")).toEqual(["qemu"]);
  });
  it("give a container lab's helper VM its hypervisors, and a VM lab its own once read", () => {
    const docker = rt({ runtime: "DOCKER" });
    expect(localProviders(docker)).toEqual([]);
    expect(localProviders(docker, "x86_64", options({ dockerVm: [path("docker-vm", "utm")] }))).toEqual(["utm"]);
    const vm = rt({ architectures: ["x86_64"], providers: ["virtualbox", "utm"] as Provider[] });
    expect(localProviders(vm, "x86_64", options({ vms: [path("vagrant", "virtualbox")] }))).toEqual(["virtualbox"]);
  });
});

describe("hostSupports", () => {
  it("needs the lab to list the host's provider", () => {
    expect(hostSupports(rt({ runtime: "DOCKER", providers: ["aws"] as Provider[] }), host("aws"))).toBe(true);
    expect(hostSupports(rt({ runtime: "DOCKER", providers: ["aws"] as Provider[] }), host("gcp"))).toBe(false);
    expect(hostSupports(null, host("aws"))).toBe(false);
  });
  it("runs a VM lab on every cloud it lists, not only AWS", () => {
    for (const cloud of ["aws", "azure", "gcp", "digitalocean", "linode", "oci"] as Provider[]) {
      expect(hostSupports(rt({ providers: [cloud] }), host(cloud)), cloud).toBe(true);
    }
    expect(hostSupports(rt({ providers: ["aws"] as Provider[] }), host("azure"))).toBe(false);
  });
  it("goes by the backend's run options once read: its servers, its clouds in any form", () => {
    const lab = rt({ providers: ["vmware_esxi", "proxmox", "aws"] as Provider[] });
    const read = options({ servers: [path("vagrant", "vmware_esxi")], cloudVm: [path("cloud-vm", "azure")] });
    expect(hostSupports(lab, host("vmware_esxi"), read)).toBe(true);
    expect(hostSupports(lab, host("proxmox"), read)).toBe(false);
    expect(hostSupports(lab, host("azure"), read)).toBe(true);
    expect(hostSupports(lab, host("aws"), read)).toBe(false);
  });
});

describe("runPlaces", () => {
  // The places the lab supports at all (ready or configurable), as before the tri-state.
  const available = (places: ReturnType<typeof runPlaces>) => places.filter((p) => p.state !== "unsupported").map((p) => p.key);
  const stateOf = (places: ReturnType<typeof runPlaces>, key: string) => places.find((p) => p.key === key)?.state;
  it("lists a container lab's places", () => {
    expect(available(runPlaces(rt({ runtime: "DOCKER", providers: ["docker", "aws"] as Provider[] })))).toEqual(["container", "local_vm", "cloud"]);
  });
  it("offers a foreign-CPU VM lab here only with an emulator", () => {
    const lab = rt({ architectures: ["x86_64"], providers: ["virtualbox", "proxmox"] as Provider[] });
    expect(available(runPlaces(lab, "arm64"))).toEqual(["server"]);
    expect(available(runPlaces(lab, "arm64", true))).toEqual(["local_vm", "server"]);
  });
  it("offers both container and VM for a dual-form lab (docker + vm flags)", () => {
    // `runtime` stays the published primary; the flags say it runs both ways, so every place its
    // providers allow is offered.
    const lab = rt({ runtime: "DOCKER", docker: true, vm: true, providers: ["virtualbox", "proxmox", "aws"] as Provider[] });
    expect(available(runPlaces(lab))).toEqual(["container", "local_vm", "server", "cloud"]);
  });
  it("marks a supported place configurable until the launcher is set up for it, then ready", () => {
    const lab = rt({ runtime: "DOCKER", docker: true, vm: true, providers: ["virtualbox", "proxmox", "aws"] as Provider[] });
    // Nothing set up: a container runs now; the VM, server and cloud it supports are configurable.
    const bare = runPlaces(lab);
    expect(stateOf(bare, "container")).toBe("ready");
    expect(stateOf(bare, "local_vm")).toBe("configurable");
    expect(stateOf(bare, "server")).toBe("configurable");
    expect(stateOf(bare, "cloud")).toBe("configurable");
    // A ready hypervisor, a Proxmox server and an AWS account: those places turn ready.
    const setUp = runPlaces(lab, undefined, false, {
      readyVms: ["virtualbox"] as Provider[],
      hosts: [host("proxmox"), host("aws")],
      options: options({ dockerVm: [path("docker-vm", "virtualbox")], servers: [path("docker-vm", "proxmox")], cloudDocker: [path("cloud-docker", "aws")] }),
    });
    expect(stateOf(setUp, "local_vm")).toBe("ready");
    expect(stateOf(setUp, "server")).toBe("ready");
    expect(stateOf(setUp, "cloud")).toBe("ready");
    // A place the lab doesn't support stays unsupported, whatever the setup.
    const localOnly = rt({ runtime: "DOCKER", docker: true, providers: ["virtualbox"] as Provider[] });
    expect(stateOf(runPlaces(localOnly, undefined, false, { hosts: [host("proxmox")] }), "server")).toBe("unsupported");
  });
  it("takes each place from the run options once loaded: a server, cloud or Docker not in them is unsupported", () => {
    // Regression: only this machine's VM place read the run options; the catalogue listing
    // Proxmox and AWS kept those places offered though the backend gave the lab neither.
    const lab = rt({ runtime: "DOCKER", docker: true, vm: true, providers: ["virtualbox", "proxmox", "aws"] as Provider[] });
    const loaded = runPlaces(lab, undefined, false, { hosts: [host("proxmox"), host("aws")], options: options({ docker: null }) });
    expect(stateOf(loaded, "container")).toBe("unsupported");
    expect(stateOf(loaded, "server")).toBe("unsupported");
    expect(stateOf(loaded, "cloud")).toBe("unsupported");
    // Cloud services Isoloom refuses on their cloud: nowhere.
    const goat = rt({ runtime: "CLOUD", cloud: true, providers: ["aws"] as Provider[] });
    expect(stateOf(runPlaces(goat, undefined, false, { options: options({}) }), "cloud")).toBe("unsupported");
    expect(stateOf(runPlaces(goat), "cloud")).toBe("configurable");
  });
});
