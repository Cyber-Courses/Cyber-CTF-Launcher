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
  it("reads the settings from the core, once", async () => {
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
