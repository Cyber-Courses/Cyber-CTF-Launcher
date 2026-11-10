import { describe, expect, it } from "vitest";
import { badgeOf, pathOf } from "@/features/labs/run-on-badges";
import type { LabRunOptions, LabRunPath, Provider } from "@/lib/tauri";

/** A launch path, its evidence naming it. */
const path = (name: string, provider: Provider, badge: LabRunPath["badge"] = "none"): LabRunPath => ({ provider, badge, evidence: `${name} on ${provider}` });
const options = {
  docker: null,
  dockerVm: [path("docker-vm", "virtualbox", "beta")],
  vms: [path("vagrant", "qemu")],
  servers: [path("proxmox", "proxmox")],
  cloudDocker: [path("cloud-docker", "aws")],
  cloudVm: [path("cloud-vm", "aws")],
  cloudServices: [],
  refused: [],
  emulated: false,
} as unknown as LabRunOptions;

describe("pathOf", () => {
  it("takes a helper VM's path and one VM per machine's from their own lists, on the target's hypervisor", () => {
    expect(pathOf(options, { kind: "local-vm", provider: "virtualbox" })?.evidence).toMatch(/^docker-vm /);
    expect(pathOf(options, { kind: "local-vms", provider: "qemu" })?.evidence).toMatch(/^vagrant /);
    // Not offered on that hypervisor: no path.
    expect(pathOf(options, { kind: "local-vm", provider: "qemu" })).toBeUndefined();
    expect(pathOf(options, { kind: "local-vms", provider: "virtualbox" })).toBeUndefined();
  });

  it("takes a server's by its kind and a cloud account's in the form picked, and none here or before the options", () => {
    expect(pathOf(options, { kind: "host", id: "pve" }, "proxmox")?.evidence).toMatch(/^proxmox /);
    expect(pathOf(options, { kind: "host", id: "a" }, "aws")?.evidence).toMatch(/^cloud-docker /);
    expect(pathOf(options, { kind: "host", id: "a", perMachine: true }, "aws")?.evidence).toMatch(/^cloud-vm /);
    expect(pathOf(options, { kind: "local" })).toBeUndefined();
    expect(pathOf(null, { kind: "local-vm", provider: "virtualbox" })).toBeUndefined();
  });
});

describe("badgeOf", () => {
  it("badges a path that isn't proven, with its evidence, and none for a proven one", () => {
    expect(badgeOf(pathOf(options, { kind: "local-vm", provider: "virtualbox" }))).toEqual({ kind: "beta", title: "docker-vm on virtualbox" });
    expect(badgeOf(pathOf(options, { kind: "local-vms", provider: "qemu" }))).toBeUndefined();
  });
});
