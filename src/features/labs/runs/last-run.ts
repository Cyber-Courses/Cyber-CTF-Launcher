import { changeSettings, settingsNow } from "@/lib/settings-store";
import type { LabPlacement } from "@/lib/tauri";

/** Per-lab "last launched" times and places, kept by the core with the settings: rows show the
 *  recency, and a start with nothing newly picked goes back to the place (the player's own
 *  earlier choice). */

/** Notes that lab `id` was started now: at `place`, the placement its launch was sent with; none
 *  for a resume, which brings it back where it was. */
export const setLastRun = (id: string, place: LabPlacement | null = null) => {
  changeSettings({ setting: "lastRun", value: { lab: id, at: Date.now(), place } });
};

export const getLastRun = (id: string): number | null => settingsNow().lastRuns[id] ?? null;

/** Where lab `id` was last started here; null when it never was (since this was kept). */
export const getLastPlace = (id: string): LabPlacement | null => settingsNow().lastPlaces[id] ?? null;

/** Forgets a lab's "last launched" time and place: the lab was removed from this computer. */
export const forgetLastRun = (id: string) => {
  changeSettings({ setting: "lastRun", value: { lab: id, at: null, place: null } });
};
