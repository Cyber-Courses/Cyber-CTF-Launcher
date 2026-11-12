// The desktop end-to-end's plan, with nothing started: the cases (cases.tsv), what each spec
// declares it needs (`export const needs = [...]` in specs/<spec>.mjs) and provides, the groups a
// run splits into, and the verdicts that follow from a need not met. tests/desktop/run.mjs runs
// the plan; tests/desktop/run.test.ts holds it.
//
// A need is a word. The runner provides some itself, once, with a cap (`RUNNER_NEEDS`: the
// engine answering, the fixture lab folder, a session, a hypervisor, a server); a spec provides
// the others by passing (`provides`: `launch` proves the fixture lab starts here, so the specs that
// start it again need `running-lab:fixture`). A need not met skips every spec needing it, at once,
// with the failure as the reason; the spec that owns the failure keeps its own verdict.
import { readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * @typedef {object} Case
 * @property {string} case the case's id (its summary line's, its log's)
 * @property {string} spec the spec it runs, specs/<spec>.mjs
 * @property {string} form the form it runs a lab in ("" for none)
 * @property {string[]} knobs the knobs it reads
 * @property {string[]} groups the groups it belongs to
 * @property {string[]} needs what it needs met first
 * @property {string[]} provides what it provides by passing
 */
/** @typedef {{ cases: Case[], providers: Map<string, string> }} Plan */
/** @typedef {"ok" | "FAIL" | "skip"} Status */

/** The needs the runner provides itself, each with what it checks. */
export const RUNNER_NEEDS = {
  engine: "a Docker engine answers here",
  "fixture-lab": "the fixture lab folder (CYBERCTF_TEST_LOCAL_LAB)",
  session: "a signed-in session (CYBERCTF_TEST_ACCOUNT_SESSION)",
  hypervisor: "a hypervisor (CYBERCTF_TEST_VM_PROVIDER) that answers here",
  server: "a server's settings (CYBERCTF_TEST_SERVER_ENV)",
  "own-frame": "the app draws its own window frame (Linux, Windows)",
  unix: "a Unix signal reaches the deploy worker (not Windows)",
  "fixture-vm": "a VM named for the installed fixture lab (UTM on macOS, libvirt on Linux)",
};

/** cases.tsv's rows: each case's id, spec, form, knobs and groups (its declarations still empty).
 *  @param {string} file
 *  @returns {Case[]} */
export function readCases(file) {
  return readFileSync(file, "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => {
      const [id = "", spec = "", form = "", knobs = "", groups = ""] = l.split("\t");
      const words = (s) => (s === "-" || s === "" ? [] : s.split(" "));
      return { case: id, spec, form: form === "-" ? "" : form, knobs: words(knobs), groups: words(groups), needs: [], provides: [] };
    });
}

/**
 * What a spec's source declares, on one line each, as literals: `export const needs = [...]`
 * and `export const provides = [...]`; a spec run once per form declares each by form
 * (`{ container: [...], "docker-vm": [...] }`), and `form` picks that form's.
 * @param {string} source
 * @param {string} [form]
 * @returns {{ needs: string[], provides: string[] }}
 */
export function declarationsOf(source, form = "") {
  /** @returns {string[]} */
  const read = (name) => {
    const m = new RegExp(`^export const ${name} = (.*);$`, "m").exec(source);
    // A literal as prettier writes it: the keys it leaves unquoted, quoted again for JSON.
    /** @type {string[] | Record<string, string[]>} */
    const value = m ? JSON.parse(m[1].replace(/([{,]\s*)([A-Za-z_][\w-]*)\s*:/g, '$1"$2":')) : [];
    return Array.isArray(value) ? value : (value[form] ?? []);
  };
  return { needs: read("needs"), provides: read("provides") };
}

/** The plan: the cases with their declarations, from `cases.tsv` and `specs/` under `dir`.
 *  @param {string} dir
 *  @returns {Plan} */
export function readPlan(dir) {
  const cases = readCases(join(dir, "cases.tsv")).map((c) => ({ ...c, ...declarationsOf(readFileSync(join(dir, "specs", `${c.spec}.mjs`), "utf8"), c.form) }));
  return { cases, providers: providersOf(cases) };
}

/** Which case provides each spec-provided need.
 *  @param {Case[]} cases
 *  @returns {Map<string, string>} */
export function providersOf(cases) {
  const out = new Map();
  for (const c of cases) for (const need of c.provides) out.set(need, c.case);
  return out;
}

/** The groups, in the order they first appear.
 *  @param {Case[]} cases
 *  @returns {string[]} */
export const groupsOf = (cases) => [...new Set(cases.flatMap((c) => c.groups))];

/**
 * What is wrong with the plan, or nothing: every need is the runner's or a case's, each need's
 * provider comes before the cases needing it, no case needs itself, each group holds the
 * providers its cases need, and every case is in a group.
 * @param {Plan} plan
 * @returns {string[]}
 */
export function planProblems({ cases, providers }) {
  const problems = [];
  const seen = new Set();
  for (const c of cases) {
    if (c.groups.length === 0) problems.push(`${c.case} is in no group`);
    for (const need of c.needs) {
      const by = providers.get(need);
      if (!(need in RUNNER_NEEDS) && by === undefined) problems.push(`${c.case} needs ${need}, which nothing provides`);
      if (by === c.case) problems.push(`${c.case} needs ${need}, which it provides itself`);
      if (by !== undefined && by !== c.case && !seen.has(by)) problems.push(`${c.case} needs ${need}, but its provider ${by} comes after it`);
      if (by !== undefined && by !== c.case)
        for (const g of c.groups)
          if (!cases.find((p) => p.case === by)?.groups.includes(g)) problems.push(`group ${g} has ${c.case}, which needs ${need}, but not its provider ${by}`);
    }
    seen.add(c.case);
  }
  return problems;
}

/** The cases a run takes: a group's, or those `only` names (every case when neither).
 *  @param {Case[]} cases
 *  @param {{ group?: string, only?: string[] }} [selection]
 *  @returns {Case[]} */
export function selectCases(cases, { group = "", only = [] } = {}) {
  let picked = cases;
  if (group) picked = picked.filter((c) => c.groups.includes(group));
  if (only.length) picked = picked.filter((c) => only.includes(c.case));
  return picked;
}

/**
 * The state of the needs as the run goes: `unmet(need)` says why a need isn't met (null when it
 * is), through its providers; `provided(need, why)` records a runner-provided need, `verdict(case,
 * status, why)` a case's, which decides the needs it provides.
 * @param {Map<string, string>} providers
 */
export function needsState(providers) {
  /** @type {Map<string, string | null>} */
  const runner = new Map();
  /** @type {Map<string, { status: Status, why: string }>} */
  const verdicts = new Map();
  const state = {
    /** @param {string} need @param {string | null} why */
    provided: (need, why) => runner.set(need, why),
    /** @param {string} name @param {Status} status @param {string} why */
    verdict: (name, status, why) => verdicts.set(name, { status, why }),
    /** Whether the runner was asked for `need` yet. @param {string} need */
    checked: (need) => runner.has(need),
    /** Why `need` isn't met, or null. A need whose provider didn't run (left out of this run) counts as met.
     *  @param {string} need @returns {string | null} */
    unmet: (need) => {
      if (need in RUNNER_NEEDS) {
        const why = runner.get(need);
        return why ? `needs ${need}: ${why}` : null;
      }
      const by = providers.get(need);
      const v = by === undefined ? undefined : verdicts.get(by);
      if (!v || v.status === "ok") return null;
      return `needs ${need}, which ${by} didn't provide (${v.status === "skip" ? "skipped" : "failed"}: ${v.why})`;
    },
    /** The first unmet need of `c`, or null. @param {{ needs: string[] }} c @returns {string | null} */
    blocked: (c) => c.needs.map(state.unmet).find((why) => why !== null) ?? null,
  };
  return state;
}

/** The failure a dead app leaves in a spec's log: WebDriver refused (the app went away), or the
 *  app gone before its server answered. */
export const DEAD_APP = /ECONNREFUSED|app exited \(code -?\d+\) before the server came up|embedded WebDriver server never came up/;

/** A spec's check lines (`ok|FAILED|skipped: <id> (<n>s) <text>`, tests/desktop/lib/support.mjs).
 *  @param {string} log
 *  @returns {{ status: Status, id: string, seconds: number, text: string }[]} */
export function checkLines(log) {
  /** @type {{ status: Status, id: string, seconds: number, text: string }[]} */
  const out = [];
  /** @type {Record<string, Status>} */
  const status = { ok: "ok", FAILED: "FAIL", skipped: "skip" };
  for (const line of log.split("\n")) {
    const m = /^(ok|FAILED|skipped): (\S+) \((\d+)s\) (.*)$/.exec(line.replace(/\r$/, ""));
    if (m) out.push({ status: status[m[1]], id: m[2], seconds: Number(m[3]), text: m[4] });
  }
  return out;
}

/** The reason a whole spec skipped (`skipped: spec (0s) <why>`), or null.
 *  @param {string} log
 *  @returns {string | null} */
export const skipReason = (log) => checkLines(log).find((l) => l.id === "spec" && l.status === "skip")?.text ?? null;

/** A case's or a check's JSON record, as every runner writes them (tests/README.md, "Results").
 *  @param {{ name: string, status: Status, seconds: number, reason: string, form?: string, provider?: string, log?: string }} r
 *  @returns {string} */
export function jsonRecord({ name, status, seconds, reason, form = "", provider = "", log = "" }) {
  const result = { ok: "pass", FAIL: "fail", skip: "skip" }[status];
  return JSON.stringify({ suite: "desktop", case: name, provider, form, result, reason: status === "ok" ? "" : reason, seconds, log });
}

/** A line of the summary: as scripts/lib/suite.sh prints them.
 *  @param {{ name: string, status: Status, seconds: number, detail: string }} r
 *  @returns {string} */
export const summaryLine = ({ name, status, seconds, detail }) => `${name.padEnd(20)} ${status.padEnd(5)} ${String(seconds).padStart(6)}s  ${detail}`;

/** A line of an app's log that may name a secret (a password, a token, a key): left out of the
 *  copy the run keeps, as the test rig's own copier leaves it out. */
export const SECRET_LINE = /password|passwd|token|secret|private key/i;

/** `text` without the lines that may name a secret (SECRET_LINE), a line break kept at the end.
 *  @param {string} text
 *  @returns {string} */
export function withoutSecrets(text) {
  const kept = text.split("\n").filter((l) => !SECRET_LINE.test(l));
  return kept.join("\n");
}

/** The last lines of a log that say what went wrong, for a failure's reason: the app's output
 *  tail a spec printed, else the log's last lines, on one line.
 *  @param {string} log
 *  @param {number} [lines]
 *  @returns {string} */
export function failureTail(log, lines = 6) {
  const text = log.replace(/\r/g, "");
  const tail = /app out tail: ([\s\S]*)$/.exec(text)?.[1] ?? text;
  return tail
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .slice(-lines)
    .join(" | ")
    .slice(0, 600);
}
