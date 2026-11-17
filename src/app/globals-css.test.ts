import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { compile } from "tailwindcss";
import { describe, expect, it } from "vitest";

// src/app/globals.css: where Tailwind looks for the UI's classes, read through Tailwind's own
// compiler. Left to find them, Tailwind takes the whole project, and in a copy inside another
// repository that ignores it (the workspace's build/src/) Turbopack then read every file there,
// src-tauri/target's gigabytes among them, again on each file a run wrote.

const APP = import.meta.dirname;
const require = createRequire(import.meta.url);

/** globals.css compiled, its imports read from disk (`tailwindcss` from the package). */
async function compiled() {
  return compile(readFileSync(join(APP, "globals.css"), "utf8"), {
    base: APP,
    loadStylesheet: (id: string, base: string) => {
      const path = id === "tailwindcss" ? require.resolve("tailwindcss/index.css") : resolve(base, id);
      return Promise.resolve({ path, base: dirname(path), content: readFileSync(path, "utf8") });
    },
    loadModule: () => Promise.reject(new Error("globals.css loads no Tailwind plugin or config")),
  });
}

describe("the UI's stylesheet", () => {
  it("takes Tailwind's classes from src/ alone, never the whole project", async () => {
    const { root, sources } = await compiled();
    if (root === null || root === "none") throw new Error(`Tailwind finds its sources itself (root ${String(root)})`);
    expect(resolve(root.base, root.pattern)).toBe(resolve(APP, ".."));
    expect(sources).toEqual([]);
  });
});
