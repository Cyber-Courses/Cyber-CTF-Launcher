// The dev mock's settings (development only, see index.ts): the core's `settings_*` commands,
// the settings kept in this browser's storage under one key of the preview's own (kept by the
// webview, not handed over), so the preview's tabs share them as the app's windows do. A test
// starts the preview with settings by writing that key first (`MOCK_SETTINGS_KEY`).
import { applyChange } from "@/lib/settings-store";
import type { Change, Settings } from "@/lib/tauri";
import { SETTINGS_DEFAULTS } from "@/lib/tauri/generated/SettingsDefaults";
import type { Args, Handler } from "@/lib/dev-mock/types";

/** The storage key of the preview's settings: a partial `Settings` as JSON. */
export const MOCK_SETTINGS_KEY = "cyberctf.devMock.settings";

function kept(): Settings {
  try {
    const raw = localStorage.getItem(MOCK_SETTINGS_KEY);
    return raw ? { ...SETTINGS_DEFAULTS, ...(JSON.parse(raw) as Partial<Settings>) } : SETTINGS_DEFAULTS;
  } catch {
    return SETTINGS_DEFAULTS;
  }
}

function keep(settings: Settings): Settings {
  try {
    localStorage.setItem(MOCK_SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    /* no storage: the change lasts for this answer */
  }
  return settings;
}

/** The settings commands, as the core answers them (without its rules on earlier default images
 *  and remote hypervisors, which its own tests hold). */
export const settingsCommands: Record<string, Handler> = {
  settings_get: (): Settings => kept(),
  settings_change: (args: Args): Settings => keep(((args?.changes as Change[] | undefined) ?? []).reduce(applyChange, kept())),
};
