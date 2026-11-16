// The leftover audit's diff: what is in the snapshot after a run and wasn't before, kind by kind,
// sorted into what the run left (a failure), what it may leave (the allowlist, with its reason)
// and what another process made meanwhile (a shared machine's: left alone, reported). A kind the
// snapshots couldn't read (its tool missing, refusing or silent) is skipped, saying which.

import { type Item, type Snapshot, parseProbe } from "./inventory.ts";
import { type Allowance, type Scope, allowanceFor, ownedByRun } from "./scope.ts";

/** A kind's verdict, with the new things sorted. */
export type KindResult = {
  kind: string;
  result: "pass" | "fail" | "skip";
  reason: string;
  left: Item[];
  allowed: { item: Item; reason: string }[];
  others: Item[];
};

/** A record of the launcher's test entry points (tests/README.md, "Results"). */
export type ResultRecord = { suite: "audit"; case: string; provider: string; form: string; result: string; reason: string; seconds: number; log: string };

/** At most `max` names, then how many more. */
export function nameList(names: readonly string[], max = 20): string {
  const shown = names.slice(0, max).join(", ");
  return names.length > max ? `${shown} and ${String(names.length - max)} more` : shown;
}

/** Why a kind can't be compared: its probe missing from a snapshot, or not read there. */
function unreadable(kind: string, before: Snapshot, after: Snapshot): string | undefined {
  for (const [when, snap] of [
    ["before", before],
    ["after", after],
  ] as const) {
    const probe = snap.probes.find((p) => p.kind === kind);
    if (!probe) return `not in the snapshot ${when} the run`;
    if (probe.status !== "ok") return `${probe.note || probe.status} (${probe.tool}, ${when} the run)`;
  }
  return undefined;
}

/** The items of a kind in a snapshot. */
const itemsOf = (snap: Snapshot, kind: string): Item[] => snap.probes.filter((p) => p.kind === kind).flatMap((p) => parseProbe(p, snap.taken));

/** Every kind's new items, sorted, and its verdict. Kinds in the order the snapshot after lists them. */
export function diff(before: Snapshot, after: Snapshot, scope: Scope, allowlist: readonly Allowance[]): KindResult[] {
  const kinds = [...new Set([...after.probes, ...before.probes].map((p) => p.kind))];
  const fresh = new Map<string, Item[]>();
  for (const kind of kinds) {
    if (unreadable(kind, before, after)) continue;
    const had = new Set(itemsOf(before, kind).map((i) => i.key));
    fresh.set(
      kind,
      itemsOf(after, kind).filter((i) => !had.has(i.key)),
    );
  }
  // The run's processes, whose listening sockets are the run's too.
  const pids = new Set((fresh.get("process") ?? []).filter((p) => p.pid !== undefined && ownedByRun(p, scope, new Set())).map((p) => p.pid ?? 0));
  return kinds.map((kind) => {
    const why = unreadable(kind, before, after);
    if (why) return { kind, result: "skip", reason: why, left: [], allowed: [], others: [] };
    const out: KindResult = { kind, result: "pass", reason: "", left: [], allowed: [], others: [] };
    for (const item of fresh.get(kind) ?? []) {
      const allowance = allowanceFor(item, scope.mode, allowlist);
      if (allowance) out.allowed.push({ item, reason: allowance.reason });
      else if (ownedByRun(item, scope, pids)) out.left.push(item);
      else out.others.push(item);
    }
    if (out.left.length > 0) {
      out.result = "fail";
      out.reason = `left ${String(out.left.length)}: ${nameList(out.left.map((i) => i.name))}`;
    } else {
      const notes = [];
      if (out.allowed.length > 0) notes.push(`${String(out.allowed.length)} allowed (${[...new Set(out.allowed.map((a) => a.reason))].join("; ")})`);
      if (out.others.length > 0) notes.push(`${String(out.others.length)} new but not the run's, left alone`);
      out.reason = notes.join("; ");
    }
    return out;
  });
}

/** A kind's verdict as a record of suite `audit`, its case the kind. */
export const toRecord = (r: KindResult, log: string, seconds = 0): ResultRecord => ({
  suite: "audit",
  case: r.kind,
  provider: "",
  form: "",
  result: r.result,
  reason: r.reason,
  seconds,
  log,
});

/** The diff for a person: per kind, what the run left, what it may leave and why, what isn't its. */
export function report(results: readonly KindResult[], before: Snapshot, after: Snapshot, scope: Scope): string {
  const out = [
    `leftover audit of ${after.host} (${after.os}, ${scope.mode}): before ${new Date(before.taken * 1000).toISOString()}, after ${new Date(after.taken * 1000).toISOString()}`,
  ];
  if (scope.roots.length > 0) out.push(`the run's folders: ${scope.roots.join(", ")}`);
  for (const r of results) {
    const verdict = { pass: "ok", fail: "FAIL", skip: "skip" }[r.result];
    out.push(`${r.kind.padEnd(20)} ${verdict.padEnd(5)} ${r.result === "skip" ? r.reason : ""}`.trimEnd());
    for (const i of r.left) out.push(`    left:    ${i.name}`);
    for (const a of r.allowed) out.push(`    allowed: ${a.item.name} (${a.reason})`);
    for (const i of r.others) out.push(`    not the run's: ${i.name}`);
  }
  return `${out.join("\n")}\n`;
}
