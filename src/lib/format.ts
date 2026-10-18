// Formatting shared across screens, so times and sizes read the same everywhere, in the current
// language (decimal separator and units: 3.4 GB in English, 3,4 Go in French).
import { getLocale, translate } from "@/lib/i18n";

/** `n` with exactly `digits` decimals, in the current language's notation. */
function decimal(n: number, digits: number) {
  return new Intl.NumberFormat(getLocale(), { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false }).format(n);
}

/** A running step's time: 0.4s, then 12s, then 1:05. */
export function formatDuration(ms: number) {
  if (!Number.isFinite(ms)) ms = 0;
  if (ms < 10_000) return translate("common.units.seconds", { n: decimal(Math.max(0, ms) / 1000, 1) });
  return formatElapsed(ms);
}

/** Whole seconds: 42s, then 1:05. */
export function formatElapsed(ms: number) {
  // A missing or garbled time reads as none, never NaN:NaN.
  const s = Number.isFinite(ms) ? Math.max(0, Math.floor(ms / 1000)) : 0;
  return s < 60 ? translate("common.units.seconds", { n: s }) : `${String(Math.floor(s / 60))}:${String(s % 60).padStart(2, "0")}`;
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

/** A gigabyte as sizes are shown: decimal, like the disk and memory figures next to them. */
export const GB = 1e9;

/** 12, 3.4: gigabytes with one decimal under 10. */
const gigabytes = (b: number) => decimal(b / GB, b >= 10 * GB ? 0 : 1);

/** 12 GB, 3.4 GB, 512 MB, 8 kB. */
export function formatBytes(b: number) {
  if (!Number.isFinite(b)) b = 0;
  if (b >= GB) return translate("common.units.gigabytes", { n: gigabytes(b) });
  if (b >= 1e6) return translate("common.units.megabytes", { n: decimal(Math.round(b / 1e6), 0) });
  return translate("common.units.kilobytes", { n: decimal(Math.max(1, Math.round(b / 1e3)), 0) });
}

/** A share of a whole, compact so it fits a card's corner in every language: 3.4/8.0 GB (one
 *  unit when both are in GB), else 512 MB / 1 GB. */
export function formatBytesOf(used: number, total: number) {
  if (used >= GB && total >= GB) return translate("common.units.gigabytesOf", { used: gigabytes(used), total: gigabytes(total) });
  return `${formatBytes(used)} / ${formatBytes(total)}`;
}

/** 3d 4h, 5h 12m, 42m. */
export function formatUptime(secs: number) {
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  if (days) return translate("common.units.uptimeDays", { days, hours });
  if (hours) return translate("common.units.uptimeHours", { hours, minutes });
  return translate("common.units.uptimeMinutes", { minutes });
}

/** US dollars in `locale`'s notation ("$0.02", "0,02 $US"). Under a cent, two significant
 *  digits ("$0.0042") rather than "$0.00"; `whole` drops the cents (a monthly budget). */
export function formatUsd(n: number, locale: string = getLocale(), { whole = false }: { whole?: boolean } = {}) {
  const tiny = n !== 0 && Math.abs(n) < 0.01;
  const digits: Intl.NumberFormatOptions = tiny ? { maximumSignificantDigits: 2 } : whole ? { maximumFractionDigits: 0 } : {};
  return new Intl.NumberFormat(locale, { style: "currency", currency: "USD", ...digits }).format(n);
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
