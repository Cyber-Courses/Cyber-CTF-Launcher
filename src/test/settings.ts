import { MOCK_SETTINGS_KEY } from "@/lib/dev-mock/settings";
import { replace } from "@/lib/settings-store";
import type { Settings } from "@/lib/tauri";
import { SETTINGS_DEFAULTS } from "@/lib/tauri/generated/SettingsDefaults";

/** Starts a test with the player's settings `over` the defaults, as the core would answer them:
 *  in the window's copy and in the mock core's (`installTauri`'s settings commands). */
export function seedSettings(over: Partial<Settings>) {
  const settings = { ...SETTINGS_DEFAULTS, ...over };
  localStorage.setItem(MOCK_SETTINGS_KEY, JSON.stringify(settings));
  replace(settings);
}
