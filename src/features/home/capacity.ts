/** Whether this machine has enough RAM to run labs comfortably. */

import { translate } from "@/lib/i18n";

type RamLevel = "ok" | "tight" | "low";

export interface Capacity {
  level: RamLevel;
  totalGB: number;
  title: string;
  detail: string;
}

/**
 * Judge lab capacity from total RAM. Container labs plus Docker Desktop and a full
 * Exegol image want headroom; multi-VM labs want more. Rough bands:
 *  - >= 16 GB: comfortable
 *  - 8-16 GB: fine for container labs, heavy VM/full-image labs may be slow
 *  - < 8 GB: likely to struggle
 */
export function assessRam(memTotalBytes: number): Capacity {
  const totalGB = memTotalBytes / 1e9;
  const level: RamLevel = totalGB >= 16 ? "ok" : totalGB >= 8 ? "tight" : "low";
  // In the current language: called while rendering, so a language change re-assesses it.
  return { level, totalGB, title: translate(`home.capacity.${level}.title`), detail: translate(`home.capacity.${level}.detail`) };
}
