// What the leftover audit's parsers share: lines, JSON lines, blocks of `Field: value` lines,
// tables, dates.

import type { Item } from "./inventory.ts";

/** The output's lines, trimmed of a carriage return, the empty ones left out. */
export const lines = (output: string): string[] =>
  output
    .split("\n")
    .map((l) => l.replace(/\r$/, ""))
    .filter((l) => l.trim() !== "");

/** The output's JSON values, one a line (a line that isn't one left out: a tool's warning). */
export function jsonLines(output: string): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  for (const line of lines(output)) {
    try {
      const value = JSON.parse(line) as unknown;
      if (Array.isArray(value)) out.push(...(value as Record<string, unknown>[]));
      else if (value && typeof value === "object") out.push(value as Record<string, unknown>);
    } catch {
      // not a JSON line: a tool's own message
    }
  }
  return out;
}

/** The output's blocks of `Field: value` lines, a blank line or a repeated `first` field
 *  starting the next: VBoxManage's and esxcli's lists. */
export function blocks(output: string, first: string): Record<string, string>[] {
  const out: Record<string, string>[] = [];
  let current: Record<string, string> | null = null;
  for (const raw of output.split("\n")) {
    const line = raw.replace(/\r$/, "");
    const m = /^\s*([^:]+?):\s*(.*)$/.exec(line);
    if (!m) {
      if (line.trim() === "") current = null;
      continue;
    }
    const [, field, value] = m;
    if (field === first || !current) {
      current = {};
      out.push(current);
    }
    current[field] ??= value.trim();
  }
  return out.filter((b) => first in b);
}

/** A table's rows, its first line naming the columns, each value up to the next column's start
 *  (`qm list`, `utmctl list`, `prlctl list`). */
export function table(output: string): Record<string, string>[] {
  const [head, ...rows] = lines(output);
  if (!head) return [];
  const starts: { name: string; at: number }[] = [];
  for (const m of head.matchAll(/\S+(?: \S+)*/g)) starts.push({ name: m[0].trim(), at: m.index });
  return rows.map((row) => {
    const out: Record<string, string> = {};
    starts.forEach((col, i) => {
      const end = i + 1 < starts.length ? starts[i + 1].at : row.length;
      out[col.name] = row.slice(col.at, end).trim();
    });
    return out;
  });
}

/** A date as epoch seconds, or undefined (a tool's zero date, an unreadable one). */
export function epoch(text: unknown): number | undefined {
  if (typeof text !== "string" || text === "" || text.startsWith("0001-")) return undefined;
  // Docker's "2026-11-11 14:05:23 +0100 CET": the offset is enough.
  const plain = text.replace(/^(\d{4}-\d\d-\d\d) (\d\d:\d\d:\d\d) ([+-]\d\d)(\d\d) \w+$/, "$1T$2$3:$4");
  const ms = Date.parse(plain);
  return Number.isNaN(ms) ? undefined : Math.floor(ms / 1000);
}

/** The non-empty strings of `values`, once each. */
export const refs = (...values: unknown[]): string[] => [...new Set(values.filter((v): v is string => typeof v === "string" && v !== ""))];

/** An item whose key, name and only path are the same path (a folder, a file). */
export const pathItem = (kind: string, path: string): Item => ({
  kind,
  key: path,
  name: path,
  refs: refs(path.split(/[\\/]/).pop()),
  paths: [path],
});

/** The items of an output whose every line is one: the line its key and name. */
export const lineItems =
  (kind: string, keep: (line: string) => boolean = () => true) =>
  (output: string): Item[] =>
    lines(output)
      .map((l) => l.trim())
      .filter(keep)
      .map((l) => ({ kind, key: l, name: l, refs: [], paths: [] }));
