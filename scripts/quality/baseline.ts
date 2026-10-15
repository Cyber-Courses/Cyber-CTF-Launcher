// The committed baseline (quality-baseline.json) and the ratchet over it: a count may go down,
// never up. Clippy's counts are kept per platform, since `cfg(windows)` code is only checked on
// Windows; a platform without counts of its own is reported but not held to any.

import fs from "node:fs";

import { type Counts, type ExceptionCounts, total } from "./model.ts";

export type Baseline = { rules: Counts; clippy: Partial<Record<string, Counts>>; exceptions: ExceptionCounts };

/** A change against the baseline: what rose, and what fell (the baseline can be lowered). */
export type Diff = { raised: string[]; lowered: string[] };

export function readBaseline(file: string): Baseline {
  if (!fs.existsSync(file)) return { rules: {}, clippy: {}, exceptions: {} };
  return JSON.parse(fs.readFileSync(file, "utf8")) as Baseline;
}

export function writeBaseline(file: string, baseline: Baseline): void {
  fs.writeFileSync(file, `${JSON.stringify(baseline, null, 2)}\n`);
}

/** Every count of `now` compared with `before`, keyed `<group> <key> in <area>`. */
function compareCounts(group: string, now: Counts, before: Counts, diff: Diff): void {
  const keys = new Set([...Object.keys(now), ...Object.keys(before)]);
  for (const key of [...keys].sort()) {
    const areas = new Set([...Object.keys(now[key] ?? {}), ...Object.keys(before[key] ?? {})]);
    for (const area of [...areas].sort()) {
      const n = now[key]?.[area] ?? 0;
      const b = before[key]?.[area] ?? 0;
      const line = `${group} ${key} in ${area}: ${String(n)} (baseline ${String(b)})`;
      if (n > b) diff.raised.push(line);
      if (n < b) diff.lowered.push(line);
    }
  }
}

/** How the measured counts compare with the baseline. */
export function compare(now: Baseline, before: Baseline, platform: string): Diff {
  const diff: Diff = { raised: [], lowered: [] };
  compareCounts("rule", now.rules, before.rules, diff);
  const clippyBefore = before.clippy[platform];
  if (clippyBefore) compareCounts("clippy", now.clippy[platform] ?? {}, clippyBefore, diff);
  compareCounts("exception", now.exceptions, before.exceptions, diff);
  return diff;
}

/** The rules whose total over every area rose: what moving code between areas can't explain. A
 *  platform's first clippy counts are recorded, not refused: there is nothing they could rise
 *  above. */
export function raisedTotals(now: Baseline, before: Baseline, platform: string): string[] {
  const raised: string[] = [];
  const check = (group: string, n: Counts, b: Counts): void => {
    for (const rule of Object.keys(n)) {
      if (total(n[rule]) > total(b[rule])) raised.push(`${group} ${rule}: ${String(total(n[rule]))} (baseline ${String(total(b[rule]))})`);
    }
  };
  check("rule", now.rules, before.rules);
  const clippyBefore = before.clippy[platform];
  if (clippyBefore) check("clippy", now.clippy[platform] ?? {}, clippyBefore);
  const exceptions = (e: ExceptionCounts): number => Object.values(e).reduce<number>((sum, byText) => sum + total(byText), 0);
  if (exceptions(now.exceptions) > exceptions(before.exceptions))
    raised.push(`exceptions: ${String(exceptions(now.exceptions))} (baseline ${String(exceptions(before.exceptions))})`);
  return raised;
}
