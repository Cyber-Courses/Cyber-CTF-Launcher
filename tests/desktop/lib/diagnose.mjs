// What a spec says of a lab that never showed running, from what the app knows of it: the
// operations in flight and the last one's log (its run events, `run_events`) and the lab's status
// (`lab_status`). Pure: support.mjs reads them from the app, this puts them into
// words, and tests/desktop/diagnose.test.ts holds the words. Without it a timed-out wait said
// "lab never ran", whether the start was still pulling images, had failed, or had never begun.

/** How many of the deploy log's last lines a report quotes. */
const LOG_TAIL = 6;

/**
 * The last `n` non-blank lines of `log`, one string.
 * @param {string} log @param {number} [n] @returns {string}
 */
export function logTail(log, n = LOG_TAIL) {
  return String(log ?? "")
    .split("\n")
    .map((l) => l.replace(/\r$/, "").trim())
    .filter(Boolean)
    .slice(-n)
    .join(" | ");
}

/**
 * Lab `id`'s operations as its run events tell them (`run_events`: `{ cursor, event }` frames,
 * oldest first): those in flight, each at its last log line (its step), and the last one, how it
 * ended (`null` while in flight, else its error or `"ok"`), with its log's lines.
 * @param {string} id @param {{ event: Record<string, any> }[] | null | undefined} frames
 * @returns {{ operations: { labId: string, op: string, step: string | null }[],
 *             last: { op: string, ended: string | null, log: string } | null }}
 */
export function fromRunEvents(id, frames) {
  /** @type {Map<string, { labId: string, op: string, lines: string[], ended: string | null, done: boolean }>} */
  const runs = new Map();
  for (const { event: e } of Array.isArray(frames) ? frames : []) {
    if (!e || e.labId !== id) continue;
    if (e.kind === "started") runs.set(e.operation, { labId: id, op: e.op, lines: [], ended: null, done: false });
    const run = runs.get(e.operation);
    if (!run) continue;
    if (e.kind === "log") run.lines.push(String(e.line));
    if (e.kind === "ended") Object.assign(run, { done: true, ended: e.error ? (e.error.message ?? JSON.stringify(e.error)) : "ok" });
  }
  const all = [...runs.values()];
  const operations = all.filter((r) => !r.done).map((r) => ({ labId: id, op: r.op, step: r.lines.at(-1) ?? null }));
  const end = all.at(-1);
  return { operations, last: end ? { op: end.op, ended: end.ended, log: end.lines.join("\n") } : null };
}

/**
 * Lab `id`'s attack box as the app knows it, in one line for a failed check's reason: the box's
 * status (`attack_box_status`), then the runner's last operation on the lab, how it ended, and
 * the end of its log.
 * @param {string} id
 * @param {{ box: Record<string, unknown> | null, frames: { event: Record<string, any> }[] | null, error?: string | null }} seen
 * @returns {string}
 */
export function boxReport(id, { box, frames, error = null }) {
  const parts = [];
  if (box)
    parts.push(
      `the box: ${
        ["running", "reachable", "ip", "labNetwork"]
          .filter((k) => box[k] !== undefined && box[k] !== null && box[k] !== "")
          .map((k) => `${k} ${String(box[k])}`)
          .join(", ") || "nothing said"
      }`,
    );
  const { last } = fromRunEvents(id, frames);
  if (last) {
    parts.push(
      `the runner's last operation: ${last.op}, ${last.ended === null ? "still in flight" : last.ended === "ok" ? "ended well" : `failed: ${last.ended}`}`,
    );
    const tail = logTail(last.log);
    parts.push(tail ? `its log ends: ${tail}` : "no log");
  } else if (Array.isArray(frames)) parts.push("no operation of the lab's");
  if (error) parts.push(`(the app couldn't be asked everything: ${error})`);
  return `${id}: ${parts.length ? parts.join("; ") : "nothing could be read"}`;
}

/**
 * The operations in flight on lab `id`, as the app lists them.
 * @param {string} id @param {{ labId: string, op: string, step: string | null }[] | null | undefined} operations
 */
const inFlight = (id, operations) => (Array.isArray(operations) ? operations : []).filter((o) => o?.labId === id);

/**
 * Whether lab `id`'s start has ended without the lab running: nothing in flight on it, and its
 * status not running (a start that failed, or never began). A status that couldn't be read
 * (null) decides nothing: false.
 * @param {string} id
 * @param {{ status: { running?: boolean } | null, operations: { labId: string }[] | null }} seen
 * @returns {boolean}
 */
export function startEnded(id, { status, operations }) {
  return !!status && !status.running && inFlight(id, operations).length === 0;
}

/**
 * Lab `id` as the app knows it, in one line for a failed check's reason: what is in flight on it
 * and its step, its status (running, parked, its machines), the end of its deploy log, and what
 * couldn't be read.
 * @param {string} id
 * @param {{ status: { running?: boolean, parked?: unknown, machines?: { name: string, state: string }[] } | null,
 *           operations: { labId: string, op: string, step: string | null }[] | null,
 *           log: string | null, error?: string | null }} seen
 * @returns {string}
 */
export function stuckLabReport(id, { status, operations, log, error = null }) {
  const parts = [];
  const ops = inFlight(id, operations);
  if (ops.length > 0) parts.push(`in flight: ${ops.map((o) => `${o.op}${o.step ? ` at "${o.step}"` : ""}`).join(", ")}`);
  else if (Array.isArray(operations)) parts.push("no operation in flight");
  if (status) {
    const machines = Array.isArray(status.machines) ? status.machines : [];
    const state = status.running ? "running" : status.parked ? "parked" : "not running";
    parts.push(`status: ${state}${machines.length ? ` (${machines.map((m) => `${m.name} ${m.state}`).join(", ")})` : ""}`);
  }
  const tail = logTail(log);
  if (tail) parts.push(`the deploy log ends: ${tail}`);
  else if (log !== null && log !== undefined) parts.push("no deploy log");
  if (error) parts.push(`(the app couldn't be asked everything: ${error})`);
  return `${id}: ${parts.length ? parts.join("; ") : "nothing could be read"}`;
}

/**
 * How the app process ended, for a spec's report: null while it runs (or when there is none),
 * else its exit code or the signal that ended it. A spec's WebDriver calls fail with
 * `ECONNREFUSED` once the app is gone, which says nothing of why; the app's own exit, and the
 * tail of what it printed, does.
 * @param {{ exitCode: number | null, signalCode?: string | null } | null | undefined} proc
 * @returns {string | null}
 */
export function appExit(proc) {
  if (!proc) return null;
  if (proc.signalCode) return `the app was ended by ${proc.signalCode}`;
  if (proc.exitCode !== null && proc.exitCode !== undefined) return `the app exited with code ${String(proc.exitCode)}`;
  return null;
}
