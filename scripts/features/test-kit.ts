// What the feature registry's tests share: a feature, a registry and facts made in a line.
import type { Facts } from "./facts.ts";
import type { Result, RigSnapshot } from "./matrix.ts";
import type { Feature, Registry, Tier } from "./registry.ts";

/** A feature of area `labs`, every field at its default but those given. */
export function feature(over: Partial<Omit<Feature, "tests">> & { tests?: Partial<Record<Tier, string[]>> } = {}): Feature {
  return {
    id: "labs.a",
    area: "labs",
    what: "Do a thing.",
    platforms: ["linux", "macos", "windows"],
    providers: [],
    host: false,
    commands: [],
    pages: [],
    rig: [],
    gaps: {},
    note: "",
    ...over,
    tests: { unit: [], web: [], desktop: [], integration: [], setup: [], package: [], ...over.tests },
  };
}

/** A registry of one area, `labs`, holding `list`. */
export const registryOf = (list: Feature[], knownGaps: Registry["knownGaps"] = []): Registry => ({
  areas: [{ id: "labs", title: "Labs", features: list }],
  knownGaps,
});

/** Facts of a small repository: `files` its files by path, the rest as given. */
export function factsOf(files: Record<string, string>, over: Partial<Omit<Facts, "read">> = {}): Facts {
  return {
    commands: [],
    pages: [],
    scenarios: [],
    desktopCases: new Map(),
    desktopChecks: new Map(),
    setupItems: [],
    ...over,
    read: (file) => files[file] ?? null,
  };
}

/** A rig snapshot where each `[column, os, result, setup]` is one cell, alone on its row. */
export function rigOf(cells: [string, string, Result, string?][]): RigSnapshot {
  return {
    generated: "2026-01-01",
    columns: [...new Set(cells.map(([c]) => c))],
    rows: cells.map(([column, os, result, setup = "golden"]) => ({ os, setup, cells: { [column]: result } })),
  };
}
