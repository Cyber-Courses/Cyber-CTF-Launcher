// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import { installTauri, rejects } from "@/test/tauri";
import { changeSettings, loadSettings, settingsNow, subscribeSettings } from "@/lib/settings-store";
import { getPortMode, getVmProvider, setPortMode } from "@/lib/settings";
import { SETTINGS_DEFAULTS } from "@/lib/tauri/generated/SettingsDefaults";
import type { Settings } from "@/lib/tauri";

const settings = (over: Partial<Settings>): Settings => ({ ...SETTINGS_DEFAULTS, ...over });

describe("the settings' copy in a window", () => {
  it("hands over the settings 1.9 kept in localStorage, removes what the core took and keeps the window's own", async () => {
    localStorage.setItem("cyberctf.appearance", "light");
    localStorage.setItem("cyberctf.locale", "fr");
    localStorage.setItem("cyberctf.onboarded", "1");
    localStorage.setItem("cyberctf.devMock.hosts", "[]");
    localStorage.setItem("cyberctf.labs.ports", "default");
    localStorage.setItem("cyberctf.lastrun.web", "1763300000000");
    const calls = installTauri({
      settings_import_webview: (a) => {
        const items = a?.items as Record<string, string>;
        return { taken: Object.keys(items), settings: settings({ portMode: "default", lastRuns: { web: 1763300000000 } }) };
      },
    });
    await loadSettings();
    const sent = calls.find((c) => c.cmd === "settings_import_webview")?.args?.items;
    expect(sent).toEqual({ "cyberctf.labs.ports": "default", "cyberctf.lastrun.web": "1763300000000" });
    expect(getPortMode()).toBe("default");
    expect(settingsNow().lastRuns.web).toBe(1763300000000);
    expect(Object.keys(localStorage).sort()).toEqual(["cyberctf.appearance", "cyberctf.devMock.hosts", "cyberctf.locale", "cyberctf.onboarded"]);
    expect(calls.some((c) => c.cmd === "settings_get")).toBe(false);
  });

  it("only reads the settings when the window has nothing to hand over, and loads once", async () => {
    localStorage.setItem("cyberctf.appearance", "dark");
    const calls = installTauri({ settings_get: () => settings({ vmProvider: "utm" }) });
    await Promise.all([loadSettings(), loadSettings()]);
    expect(getVmProvider()).toBe("utm");
    expect(calls.filter((c) => c.cmd.startsWith("settings_")).map((c) => c.cmd)).toEqual(["settings_get"]);
  });

  it("keeps the defaults, logged, when the core can't answer", async () => {
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    installTauri({ settings_get: rejects({ code: "other", message: "the launcher's store: locked" }) });
    await loadSettings();
    expect(settingsNow()).toEqual(SETTINGS_DEFAULTS);
    expect(warned).toHaveBeenCalled();
    warned.mockRestore();
  });

  it("applies a change at once, then takes the core's answer, which follows its rules", async () => {
    const calls = installTauri({ settings_change: () => settings({ attackImage: null }) });
    changeSettings({ setting: "attackImage", value: "cyberctf/attack-box:latest" });
    expect(settingsNow().attackImage).toBe("cyberctf/attack-box:latest");
    await vi.waitFor(() => {
      expect(settingsNow().attackImage).toBeNull();
    });
    expect(calls.find((c) => c.cmd === "settings_change")?.args).toEqual({ changes: [{ setting: "attackImage", value: "cyberctf/attack-box:latest" }] });
  });

  it("keeps a change the core refused for this window, logged", async () => {
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    installTauri({ settings_change: rejects({ code: "other", message: "the launcher's store: read-only" }) });
    setPortMode("random");
    await vi.waitFor(() => {
      expect(warned).toHaveBeenCalledWith("saving a setting:", expect.anything());
    });
    expect(getPortMode()).toBe("random");
    warned.mockRestore();
  });

  it("follows a change another window made, and tells its readers", async () => {
    installTauri({ settings_get: () => SETTINGS_DEFAULTS });
    await loadSettings();
    const seen = vi.fn();
    const off = subscribeSettings(seen);
    await emit("settings-changed", settings({ dockerEngine: "orbstack" }));
    await vi.waitFor(() => {
      expect(settingsNow().dockerEngine).toBe("orbstack");
    });
    expect(seen).toHaveBeenCalled();
    off();
  });
});
