"use client";

import { useState } from "react";
import { formatDuration } from "@/lib/format";
import { useNow } from "@/lib/use-now";
import { useFormat, useT, type T } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Time since the start began (`from`: when Start was pressed, or the backend's operation was
 *  first seen), ticking; since it mounted when unknown. */
export function StartTimer({ from }: { from?: number }) {
  const [mounted] = useState(() => Date.now());
  const [now] = useNow(250);
  return <span className="font-mono tabular-nums opacity-80">{formatDuration(Math.max(0, now - (from ?? mounted)))}</span>;
}

/** "3h 58m", "58m": a time left, in hours and minutes. */
export function hoursMinutes(t: T, h: number, m: number): string {
  return h > 0 ? t("labs.duration.hm", { h, m }) : t("labs.duration.m", { m });
}

/** "Auto-stops at 19:42 · in 3h 58m" for cloud labs, as inline mono text in the warning colour. */
export function AutoStop({ at, className }: { at: number; className?: string }) {
  const t = useT();
  const format = useFormat();
  const now = Math.floor(useNow(30_000)[0] / 1000);
  const left = Math.max(0, at - now);
  const time = format.date(at * 1000, { hour: "2-digit", minute: "2-digit" });
  const h = Math.floor(left / 3600);
  const m = Math.floor((left % 3600) / 60);
  return (
    <span className={cn("font-mono text-[0.6875rem] tabular-nums text-warning", className)}>
      {t("labs.timers.autoStop", { time, left: hoursMinutes(t, h, m) })}
    </span>
  );
}
