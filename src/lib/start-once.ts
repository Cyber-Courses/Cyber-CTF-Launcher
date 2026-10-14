// One start of a thing at a time, for the app's lifetime (not a component's): a second caller
// joins the start under way instead of running another. The attack box's start goes through it:
// the lab page and the app-wide watcher can both decide to start it, and a page left and opened
// again while it starts reads it as not running.
const starting = new Map<string, Promise<void>>();

/** Runs `start` for `key`, or joins the start already under way for it. */
export function startOnce(key: string, start: () => Promise<void>): Promise<void> {
  const pending = starting.get(key);
  if (pending) return pending;
  const run = start().finally(() => starting.delete(key));
  starting.set(key, run);
  return run;
}
