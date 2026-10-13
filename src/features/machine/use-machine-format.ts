import { useFormat, useT } from "@/lib/i18n";

const GB = 1e9;

/** Sizes, "ago" times and uptimes as `@/lib/format` writes them, in the current language
 *  (decimal comma and Go/Mo/ko in French). */
export function useMachineFormat() {
  const t = useT();
  const f = useFormat();
  const fixed = (n: number, digits: number) => f.number(n, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false });
  const gb = (b: number) => fixed(b / GB, b >= 10 * GB ? 0 : 1);
  /** 12 GB, 3.4 GB, 512 MB, 8 kB. */
  const bytes = (b: number) => {
    if (b >= GB) return t("machine.format.gb", { n: gb(b) });
    if (b >= 1e6) return t("machine.format.mb", { n: fixed(Math.round(b / 1e6), 0) });
    return t("machine.format.kb", { n: fixed(Math.max(1, Math.round(b / 1e3)), 0) });
  };
  return {
    bytes,
    /** A share of a whole, compact so it fits a card's corner in every language: 19.5/32 GB
     *  (one unit when both are in GB), else 512 MB / 1 GB. */
    bytesOf(used: number, total: number) {
      if (used >= GB && total >= GB) return t("machine.format.gbOf", { used: gb(used), total: gb(total) });
      return `${bytes(used)} / ${bytes(total)}`;
    },
    /** "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago". */
    ago(at: number, now: number) {
      const s = Math.max(0, (now - at) / 1000);
      if (s < 60) return t("machine.format.justNow");
      if (s < 3600) return t("machine.format.minutesAgo", { n: Math.floor(s / 60) });
      if (s < 86400) return t("machine.format.hoursAgo", { n: Math.floor(s / 3600) });
      const d = Math.floor(s / 86400);
      return d === 1 ? t("machine.format.yesterday") : t("machine.format.daysAgo", { n: d });
    },
    /** 3d 4h, 5h 12m, 42m. */
    uptime(secs: number) {
      const d = Math.floor(secs / 86400);
      const h = Math.floor((secs % 86400) / 3600);
      const m = Math.floor((secs % 3600) / 60);
      if (d) return t("machine.format.uptimeDays", { d, h });
      if (h) return t("machine.format.uptimeHours", { h, m });
      return t("machine.format.uptimeMinutes", { m });
    },
  };
}
