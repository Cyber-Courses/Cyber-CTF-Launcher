import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// The features depend on each other one way only: a cycle (labs and cloud, machine and settings)
// made each a part of the other, so neither could change, be read or be tested alone.

const FEATURES = join(__dirname);

/** The source files under `dir`, tests left out (a test may reach into any feature). */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$|test-kit\.tsx?$/.test(name) ? [path] : [];
  });
}

/** Each feature with the other features its source imports. */
function featureGraph(): Map<string, Set<string>> {
  const graph = new Map<string, Set<string>>();
  for (const feature of readdirSync(FEATURES).filter((f) => statSync(join(FEATURES, f)).isDirectory())) {
    const uses = new Set<string>();
    for (const file of sources(join(FEATURES, feature))) {
      for (const [, other] of readFileSync(file, "utf8").matchAll(/from "@\/features\/([^/"]+)/g)) {
        if (other !== feature) uses.add(other);
      }
    }
    graph.set(feature, uses);
  }
  return graph;
}

/** A cycle through `graph`, as the features along it, or null. */
function cycleIn(graph: Map<string, Set<string>>): string[] | null {
  const state = new Map<string, "open" | "done">();
  const walk = (node: string, path: string[]): string[] | null => {
    if (state.get(node) === "open") return [...path.slice(path.indexOf(node)), node];
    if (state.get(node) === "done") return null;
    state.set(node, "open");
    for (const next of graph.get(node) ?? []) {
      const found = walk(next, [...path, node]);
      if (found) return found;
    }
    state.set(node, "done");
    return null;
  };
  for (const node of graph.keys()) {
    const found = walk(node, []);
    if (found) return found;
  }
  return null;
}

describe("features", () => {
  it("import each other one way only", () => {
    expect(cycleIn(featureGraph())).toBeNull();
  });

  it("are told apart when they form a cycle", () => {
    const graph = new Map([
      ["labs", new Set(["cloud"])],
      ["cloud", new Set(["labs"])],
      ["home", new Set(["labs"])],
    ]);
    expect(cycleIn(graph)).toEqual(["labs", "cloud", "labs"]);
  });
});
