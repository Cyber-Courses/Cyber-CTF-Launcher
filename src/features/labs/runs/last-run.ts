import { readStored, writeStored } from "@/lib/local-store";

/** Per-lab "last launched" timestamps, kept locally so rows can show recency. */

const KEY = (id: string) => `cyberctf.lastrun.${id}`;

export const setLastRun = (id: string) => {
  writeStored(KEY(id), String(Date.now()));
};

export function getLastRun(id: string): number | null {
  const v = readStored(KEY(id));
  const n = v ? Number(v) : NaN;
  return Number.isFinite(n) ? n : null;
}
