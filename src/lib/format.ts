// Formatting shared across screens, so times and sizes read the same everywhere, in the current
// language (decimal separator and units: 3.4 GB in English, 3,4 Go in French).
import { getLocale, translate } from "@/lib/i18n";

/** `n` with exactly `digits` decimals, in the current language's notation. */
function decimal(n: number, digits: number) {
  return new Intl.NumberFormat(getLocale(), { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false }).format(n);
}

/** A running step's time: 0.4s, then 12s, then 1:05. */
export function formatDuration(ms: number) {
  if (ms < 10_000) return translate("common.units.seconds", { n: decimal(Math.max(0, ms) / 1000, 1) });
  return formatElapsed(ms);
}

/** Whole seconds: 42s, then 1:05. */
export function formatElapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return s < 60 ? translate("common.units.seconds", { n: s }) : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago". */
export function formatAgo(at: number, now: number) {
  const s = Math.max(0, (now - at) / 1000);
  if (s < 60) return translate("common.time.justNow");
  if (s < 3600) return translate("common.time.minutesAgo", { count: Math.floor(s / 60) });
  if (s < 86400) return translate("common.time.hoursAgo", { count: Math.floor(s / 3600) });
  const d = Math.floor(s / 86400);
  return d === 1 ? translate("common.time.yesterday") : translate("common.time.daysAgo", { count: d });
}

const GB = 1e9;

/** 12 GB, 3.4 GB, 512 MB, 8 kB. */
export function formatBytes(b: number) {
  if (b >= GB) return translate("common.units.gigabytes", { n: decimal(b / GB, b >= 10 * GB ? 0 : 1) });
  if (b >= 1e6) return translate("common.units.megabytes", { n: Math.round(b / 1e6) });
  return translate("common.units.kilobytes", { n: Math.max(1, Math.round(b / 1e3)) });
}

/** A lab id made readable for when its title isn't known (the catalogue unreachable):
 *  "lab-goad" reads "Goad", "goad-light" reads "Goad Light". */
export function prettyLabId(id: string) {
  const words = id
    .replace(/^lab[-_]/i, "")
    .split(/[-_\s]+/)
    .filter(Boolean);
  if (!words.length) return id;
  return words.map((w) => w[0].toUpperCase() + w.slice(1)).join(" ");
}
