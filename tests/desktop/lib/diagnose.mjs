// What a spec says of a lab that never showed running, from what the app knows of it: the
// operations in flight (`active_operations`), the lab's status (`lab_status`) and the end of its
// deploy log (`lab_deploy_log`). Pure: support.mjs reads them from the app, this puts them into
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
