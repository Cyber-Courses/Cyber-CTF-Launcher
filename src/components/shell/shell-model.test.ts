import { describe, expect, it } from "vitest";
import type { ActiveOperation, LabStatus, SystemReport } from "@/lib/tauri";
import { activeLabs, breadcrumbs, engineSummary, isNavTab, isTyping, shortcutFor, type Tab } from "@/components/shell/shell-model";

const title = (tab: Tab) => tab.toUpperCase();
const key = (key: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey", boolean>> = {}) => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("shell navigation", () => {
  it("knows the sidebar's screens", () => {
    expect(isNavTab("machine")).toBe(true);
    expect(isNavTab("events")).toBe(true);
    // Settings has its own window and "setup" lands on Machine: neither is a sidebar entry.
    expect(isNavTab("settings")).toBe(false);
    expect(isNavTab("setup")).toBe(false);
    expect(isNavTab("nope")).toBe(false);
  });

  it("builds the breadcrumb", () => {
    expect(breadcrumbs("labs", "Juice Shop", title, "This machine")).toEqual(["LABS", "Juice Shop"]);
    expect(breadcrumbs("labs", null, title, "This machine")).toEqual(["This machine", "LABS"]);
    expect(breadcrumbs("settings", "ignored", title, "This machine")).toEqual(["SETTINGS", "SETTINGS"]);
    expect(breadcrumbs("cloud", null, title, "This machine")).toEqual(["This machine", "CLOUD"]);
  });
});

describe("engineSummary", () => {
  const report = (patch: Partial<SystemReport>) =>
    ({
      docker: { installed: true, version: "Docker version 29.5.3, build d1c06ef" },
      dockerRunning: true,
      dockerEngine: "orbstack",
      vmProviders: [],
      ...patch,
    }) as unknown as SystemReport;

  it("names the running engine with its version", () => {
    expect(engineSummary(report({})).engine).toBe("OrbStack 29.5.3");
    expect(engineSummary(report({ docker: { installed: true, version: null } })).engine).toBe("OrbStack");
  });

  it("has no engine line while Docker is down or unidentified", () => {
    expect(engineSummary(report({ dockerRunning: false })).engine).toBeNull();
    expect(engineSummary(report({ dockerEngine: null })).engine).toBeNull();
  });

  it("names the first local hypervisor that can run", () => {
    const vmProviders = [
      { provider: "proxmox", remote: true, available: true, hypervisor: null },
      { provider: "virtualbox", remote: false, available: true, hypervisor: false },
      { provider: "qemu", remote: false, available: true, hypervisor: true },
    ] as SystemReport["vmProviders"];
    expect(engineSummary(report({ vmProviders })).hypervisor).toBe("QEMU");
    expect(engineSummary(report({})).hypervisor).toBeNull();
  });
});

describe("activeLabs", () => {
  const labs = ["a", "b", "c", "d"].map((id) => ({ id, slug: id, title: id.toUpperCase(), category: "web" }));
  const status = (running: boolean) => ({ running }) as LabStatus;

  it("lists deploying, busy and scanned-running labs, in catalogue order", () => {
    const op: ActiveOperation = { labId: "c", op: "pause", machine: null, step: "Saving state" };
    const out = activeLabs(labs, new Set(["b"]), new Map([["c", op]]), new Set(["d"]), {});
    expect(out.map((l) => l.id)).toEqual(["b", "c", "d"]);
    // A backend deploy without a live operation reads as a launch.
    expect(out[0].op).toEqual({ labId: "b", op: "launch", machine: null, step: null });
    expect(out[1].op).toBe(op);
    expect(out[2].op).toBeNull();
  });

  it("keeps the labs the list doesn't have (the catalogue unreachable), by id", () => {
    const op: ActiveOperation = { labId: "lab-goad", op: "launch", machine: "SRV02", step: "Configure SRV02" };
    const out = activeLabs([], new Set(["lab-goad"]), new Map([["lab-goad", op]]), new Set(["lab-invoice", "lab-gone"]), { "lab-gone": status(false) });
    expect(out.map((l) => [l.id, l.title, l.slug])).toEqual([
      ["lab-goad", "Goad", ""],
      ["lab-invoice", "Invoice", ""],
    ]);
    expect(out[0].op).toBe(op);
  });

  it("lets a lab's own status overrule the workload scan", () => {
    const out = activeLabs(labs, new Set(), new Map(), new Set(["a", "d"]), { a: status(false), d: status(true) });
    expect(out.map((l) => l.id)).toEqual(["d"]);
  });
});

describe("shortcutFor", () => {
  it("toggles the palette with Cmd/Ctrl+K, even while typing", () => {
    expect(shortcutFor(key("k", { metaKey: true }), true, true)).toBe("palette");
    expect(shortcutFor(key("K", { ctrlKey: true }), false, false)).toBe("palette");
    expect(shortcutFor(key("k"), false, false)).toBeNull();
  });

  it("opens Settings with Ctrl+, outside macOS only", () => {
    expect(shortcutFor(key(",", { ctrlKey: true }), false, false)).toBe("settings");
    expect(shortcutFor(key(",", { metaKey: true }), true, false)).toBeNull();
  });

  it("jumps to the lab search with a bare / outside text fields", () => {
    expect(shortcutFor(key("/"), true, false)).toBe("findLab");
    expect(shortcutFor(key("/"), true, true)).toBeNull();
    expect(shortcutFor(key("/", { altKey: true }), true, false)).toBeNull();
  });

  it("treats inputs, text areas and editable content as typing", () => {
    expect(isTyping(null)).toBe(false);
    expect(isTyping({ tagName: "INPUT" } as unknown as Element)).toBe(true);
    expect(isTyping({ tagName: "TEXTAREA" } as unknown as Element)).toBe(true);
    expect(isTyping({ tagName: "DIV", isContentEditable: true } as unknown as Element)).toBe(true);
    expect(isTyping({ tagName: "BUTTON", isContentEditable: false } as unknown as Element)).toBe(false);
  });
});
