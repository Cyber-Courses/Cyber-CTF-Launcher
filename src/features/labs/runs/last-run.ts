import { changeSettings, settingsNow } from "@/lib/settings-store";

/** Per-lab "last launched" times, kept by the core with the settings so rows can show recency. */

export const setLastRun = (id: string) => {
  changeSettings({ setting: "lastRun", value: { lab: id, at: Date.now() } });
};

export const getLastRun = (id: string): number | null => settingsNow().lastRuns[id] ?? null;

/** Forgets a lab's "last launched" time: the lab was removed from this computer. */
export const forgetLastRun = (id: string) => {
  changeSettings({ setting: "lastRun", value: { lab: id, at: null } });
};
