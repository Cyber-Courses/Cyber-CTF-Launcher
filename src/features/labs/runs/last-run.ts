/** Per-lab "last launched" timestamps, kept locally so rows can show recency. */

const KEY = (id: string) => `cyberctf.lastrun.${id}`;

export function setLastRun(id: string) {
  try {
    localStorage.setItem(KEY(id), String(Date.now()));
  } catch {
    /* ignore */
  }
}

export function getLastRun(id: string): number | null {
  try {
    const v = localStorage.getItem(KEY(id));
    const n = v ? Number(v) : NaN;
    return Number.isFinite(n) ? n : null;
  } catch {
    return null;
  }
}
