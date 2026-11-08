import { describe, expect, it } from "vitest";
import { placeName, placeOf, placeText } from "@/features/labs/place";
import { labStatus } from "@/features/labs/labs-test-kit";
import type { MessageKey, T, Vars } from "@/lib/i18n";
import type { ServerHost } from "@/lib/tauri";

// Where a lab runs, as the header, Details and the diagram's host card all say it.

// Labels as their keys, with the variables appended, so a test reads what was asked for.
const t = ((key: MessageKey, vars?: Vars) => (vars ? `${key}:${JSON.stringify(vars)}` : key)) as unknown as T;
const host = (p: Partial<ServerHost>): ServerHost =>
  ({ id: "h", name: "homelab-pve", provider: "proxmox", host: "10.0.0.5", port: 8006, username: "root@pam", ...p }) as ServerHost;
const hosts = [host({}), host({ id: "aws1", name: "My AWS", provider: "aws", host: "eu-west-3", username: "AKIA…" })];

describe("where a lab runs", () => {
  it("a container lab here runs on the engine recorded at launch, else the machine's current one", () => {
    const pinned = placeOf(labStatus({ running: true, place: "container", provider: "orbstack" }), true, "docker-desktop", hosts);
    expect(pinned).toEqual({ kind: "engine", engine: "orbstack" });
    expect(placeText(t, pinned!)).toBe('labs.detail.thisMachineWith:{"engine":"OrbStack"}');
    // "docker": the CLI's current engine, as the machine report names it.
    const current = placeOf(labStatus({ running: true, place: "container", provider: "docker" }), true, "docker-desktop", hosts);
    expect(current).toEqual({ kind: "engine", engine: "docker-desktop" });
    expect(placeName(current!)).toBe("Docker Desktop");
    // The report not in yet: this machine, unnamed.
    const unknown = placeOf(labStatus({ running: true, place: "container", provider: "docker" }), true, null, hosts);
    expect(placeText(t, unknown!)).toBe("labs.detail.thisMachine");
    expect(placeOf(undefined, true, "docker-desktop", hosts)).toBeNull();
  });

  it("a VM lab here runs on its hypervisor; a container lab in a VM here says both", () => {
    const vm = placeOf(labStatus({ running: true, place: "local_vm", provider: "qemu" }), false, "docker-desktop", hosts);
    expect(vm).toEqual({ kind: "hypervisor", provider: "qemu", inVm: false });
    expect(placeText(t, vm!)).toBe('labs.detail.thisMachineWith:{"engine":"QEMU"}');
    const inVm = placeOf(labStatus({ running: true, place: "local_vm", provider: "utm" }), true, "docker-desktop", hosts);
    expect(inVm).toEqual({ kind: "hypervisor", provider: "utm", inVm: true });
    expect(placeText(t, inVm!)).toBe('labs.detail.aVmHereWith:{"hypervisor":"UTM"}');
    // An unnamed hypervisor: the place words alone.
    expect(placeText(t, { kind: "hypervisor", provider: "", inVm: true })).toBe("labs.detail.aVmHere");
  });

  it("a lab on a server names the saved host and its kind; one whose host isn't saved any more, the name alone", () => {
    const server = placeOf(labStatus({ running: true, place: "server", host: "homelab-pve", provider: "proxmox" }), false, null, hosts);
    expect(server).toEqual({ kind: "server", name: "homelab-pve", provider: "proxmox", address: "10.0.0.5" });
    expect(placeText(t, server!)).toBe('labs.detail.serverWith:{"name":"homelab-pve","kind":"Proxmox VE"}');
    const gone = placeOf(labStatus({ running: true, place: "server", host: "old-esxi" }), false, null, hosts);
    expect(gone).toEqual({ kind: "server", name: "old-esxi", provider: null, address: null });
    expect(placeText(t, gone!)).toBe("old-esxi");
  });

  it("a lab in a cloud account names the cloud and its region", () => {
    const cloud = placeOf(labStatus({ running: true, place: "cloud", host: "My AWS", provider: "aws" }), false, null, hosts);
    expect(cloud).toEqual({ kind: "cloud", name: "My AWS", provider: "aws", region: "eu-west-3" });
    expect(placeName(cloud!)).toBe("AWS");
    expect(placeText(t, cloud!)).toBe('labs.detail.cloudWith:{"cloud":"AWS","region":"eu-west-3"}');
  });
});
