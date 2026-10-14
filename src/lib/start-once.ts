// One start of a thing at a time, for the app's lifetime (not a component's): a second caller
// joins the start under way instead of running another, and sees its log too (the lines so far,
// then the rest). The attack box's start goes through it: the lab page and the app-wide watcher
// can both decide to start it, and a page left and opened again while it starts reads it as not
// running.
type Log = (line: string) => void;
type Start = { run: Promise<void>; lines: string[]; listeners: Set<Log> };

const starting = new Map<string, Start>();

/** Runs `start` for `key` (its log to `onLog`), or joins the start already under way for it. */
export function startOnce(key: string, start: (log: Log) => Promise<void>, onLog: Log = () => {}): Promise<void> {
  const pending = starting.get(key);
  if (pending) {
    pending.lines.forEach(onLog);
    pending.listeners.add(onLog);
    return pending.run;
  }
  const entry: Start = { run: Promise.resolve(), lines: [], listeners: new Set([onLog]) };
  const log = (line: string) => {
    entry.lines.push(line);
    entry.listeners.forEach((l) => l(line));
  };
  entry.run = start(log).finally(() => starting.delete(key));
  starting.set(key, entry);
  return entry.run;
}
