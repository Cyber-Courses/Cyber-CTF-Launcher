import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { CATALOGS } from "@/messages";

const { en, ...OTHERS } = CATALOGS;

/** Every leaf path of a message tree ({ one, other } plurals count as one leaf). */
function leaves(node: unknown, prefix = ""): string[] {
  if (typeof node === "string") return [prefix];
  if (typeof node !== "object" || node === null) return [];
  const o = node as Record<string, unknown>;
  if (typeof o.one === "string" && typeof o.other === "string") return [prefix];
  return Object.entries(o).flatMap(([k, v]) => leaves(v, prefix ? `${prefix}.${k}` : k));
}
const placeholders = (s: string) => (s.match(/\{\w+\}|<\w+>/g) ?? []).map((p) => p.replace(/[{}<>]/g, "")).sort();
const at = (tree: unknown, path: string) => path.split(".").reduce<unknown>((n, p) => (n as Record<string, unknown> | undefined)?.[p], tree);
const texts = (v: unknown): string[] => (typeof v === "string" ? [v] : Object.values(v as Record<string, string>));

describe("messages", () => {
  for (const [lang, tree] of Object.entries(OTHERS)) {
    it(`${lang} has every English message, and nothing English doesn't`, () => {
      expect(leaves(tree).sort()).toEqual(leaves(en).sort());
    });

    it(`${lang} keeps the same placeholders and tags`, () => {
      for (const path of leaves(en)) {
        const e = texts(at(en, path)).flatMap(placeholders);
        const f = texts(at(tree, path)).flatMap(placeholders);
        expect(new Set(f), `${lang} ${path}`).toEqual(new Set(e));
      }
    });
  }

  it("no em dashes (house style)", () => {
    for (const tree of [en, ...Object.values(OTHERS)])
      for (const path of leaves(tree)) for (const s of texts(at(tree, path))) expect(s.includes("\u2014"), path).toBe(false);
  });
});

/** Every source file of the app (its tests and the catalogs themselves left out). */
function appSources(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return e.name === "messages" ? [] : appSources(p);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

describe("message use", () => {
  it("every English message is used by the app: its key written out, or under a prefix a key is built from", () => {
    const src = fileURLToPath(new URL("../..", import.meta.url));
    const code = appSources(src).map((f) => fs.readFileSync(f, "utf8"));
    // `"labs.attackBox.start"`, and `t(\`labs.places.${key}\`)`'s "labs.places.".
    const written = new Set(code.flatMap((c) => [...c.matchAll(/["'`]([a-z]\w*(?:\.[\w-]+)+)["'`]/g)].map((m) => m[1])));
    const prefixes = [...new Set(code.flatMap((c) => [...c.matchAll(/`([a-z]\w*(?:\.[\w-]+)*\.)\$\{/g)].map((m) => m[1])))];
    const unused = leaves(en).filter((key) => !written.has(key) && !prefixes.some((p) => key.startsWith(p)));
    expect(unused).toEqual([]);
  });
});
