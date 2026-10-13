import { describe, expect, it } from "vitest";
import type { ActiveOperation, LabStatus, SystemReport } from "@/lib/tauri";
import { activeLabs, breadcrumbs, engineSummary, isNavTab, shortcutFor, type SidebarLab, type Tab } from "@/components/shell/shell-model";

const title = (tab: Tab) => tab.toUpperCase();
const key = (key: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey", boolean>> = {}) => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});
const labs: SidebarLab[] = ["a", "b", "c"].map((id) => ({ id, slug: id, title: id, category: "Web" }));
const op = (labId: string, o: ActiveOperation["op"] = "pause"): ActiveOperation => ({ labId, op: o, machine: null, step: null });
const status = (running: boolean) => ({ running }) as LabStatus;
const report = (patch: Partial<SystemReport>) =>
  ({ docker: { installed: true, version: null }, dockerRunning: true, dockerEngine: null, vmProviders: [], ...patch }) as SystemReport;

describe("activeLabs: edges", () => {
  it("lists labs in catalogue order whatever the order of the sets, and an operation wins over the synthetic launch", () => {
    const out = activeLabs(labs, new Set(["c", "a"]), new Map([["a", op("a")]]), new Set(), {});
    expect(out.map((l) => l.id)).toEqual(["a", "c"]);
    expect(out[0].op?.op).toBe("pause");
    expect(out[1].op).toEqual({ labId: "c", op: "launch", machine: null, step: null });
  });

  it("an operation lists a lab even when its status says stopped; a running status alone never does", () => {
    const out = activeLabs(labs, new Set(), new Map([["a", op("a", "shutdown")]]), new Set(), { a: status(false), b: status(true) });
    expect(out.map((l) => l.id)).toEqual(["a"]);
  });

  it("a scanned lab without a status yet counts as running", () => {
    expect(activeLabs(labs, new Set(), new Map(), new Set(["b"]), {}).map((l) => l.id)).toEqual(["b"]);
  });
});

describe("engineSummary: edges", () => {
  it("reads a prerelease version, none from a bare major, and names Podman", () => {
    expect(engineSummary(report({ docker: { installed: true, version: "Docker version 29.5.3-rc1, build x" }, dockerEngine: "docker-desktop" })).engine).toBe(
      "Docker Desktop 29.5.3",
    );
    expect(engineSummary(report({ docker: { installed: true, version: "Docker version 29" }, dockerEngine: "docker-desktop" })).engine).toBe("Docker Desktop");
    expect(engineSummary(report({ docker: { installed: true, version: "29.5.3" }, dockerEngine: "podman" })).engine).toBe("Podman 29.5.3");
  });

  it("counts an unprobed hypervisor (Hyper-V) as ready, names an unknown one by its id, and skips remote ones", () => {
    const hyperv = { provider: "hyperv", remote: false, available: true, hypervisor: null } as SystemReport["vmProviders"][number];
    const esxi = { provider: "vmware_esxi", remote: true, available: true, hypervisor: null } as SystemReport["vmProviders"][number];
    const odd = { provider: "weird", remote: false, available: true, hypervisor: true } as unknown as SystemReport["vmProviders"][number];
    expect(engineSummary(report({ vmProviders: [esxi, hyperv] })).hypervisor).toBe("Hyper-V");
    expect(engineSummary(report({ vmProviders: [odd] })).hypervisor).toBe("weird");
    expect(engineSummary(report({ vmProviders: [esxi] })).hypervisor).toBeNull();
  });
});

describe("navigation and shortcuts: edges", () => {
  it("the Labs breadcrumb falls back to the screen when the lab title is empty", () => {
    expect(breadcrumbs("labs", "", title, "This machine")).toEqual(["This machine", "LABS"]);
    expect(breadcrumbs("labs", null, title, "This machine")).toEqual(["This machine", "LABS"]);
  });

  it("Settings is a screen but not a sidebar tab; a coming-soon tab still is one", () => {
    expect(isNavTab("settings")).toBe(false);
    expect(isNavTab("events")).toBe(true);
    expect(isNavTab("")).toBe(false);
  });

  it("Alt with / or K is nothing, Cmd+, counts as Settings off a Mac, Ctrl+, on a Mac does not", () => {
    expect(shortcutFor(key("/", { altKey: true }), false, false)).toBeNull();
    expect(shortcutFor(key("k", { altKey: true }), false, false)).toBeNull();
    expect(shortcutFor(key(",", { metaKey: true }), false, false)).toBe("settings");
    expect(shortcutFor(key(",", { ctrlKey: true }), true, false)).toBeNull();
    // On a Mac the palette is Cmd+K only (Ctrl+K is the text fields' "delete to end of line").
    expect(shortcutFor(key("K", { ctrlKey: true, altKey: true }), true, true)).toBeNull();
    expect(shortcutFor(key("K", { metaKey: true, altKey: true }), true, true)).toBe("palette");
    expect(shortcutFor(key("k", { metaKey: true }), false, false)).toBeNull();
  });
});
