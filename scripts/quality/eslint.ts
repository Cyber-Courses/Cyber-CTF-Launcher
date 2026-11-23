// ESLint over what `pnpm lint` lints (the frontend, these scripts, the end-to-end tests), through
// its API: a warning is a finding (counted by the ratchet), an error fails the run. The
// `eslint-disable` comments are the local exceptions.

import fs from "node:fs";
import path from "node:path";

import { ESLint } from "eslint";

import type { Exception, Finding } from "./model.ts";

/** What ESLint lints, relative to the repository root: the folders `pnpm lint` names
 *  (package.json's `eslint <folder>…`). */
export function linted(packageJson: string): string[] {
  const lint = (JSON.parse(packageJson) as { scripts?: { lint?: string } }).scripts?.lint ?? "";
  return lint.split(/\s+/).filter((word, i) => i > 0 && !word.startsWith("-"));
}

// Built from parts, so this line isn't one.
const DISABLE = new RegExp(["(?<=(//|/\\*)\\s*)eslint", "-disable(-next-line|-line)?\\b[^*\\n]*"].join(""));

/** The warnings and the formatted errors. */
export async function runEslint(root: string): Promise<{ warnings: Finding[]; errors: string }> {
  const eslint = new ESLint({ cwd: root });
  const results = await eslint.lintFiles(linted(fs.readFileSync(path.join(root, "package.json"), "utf8")));
  const warnings: Finding[] = [];
  for (const result of results) {
    const file = path.relative(root, result.filePath).split(path.sep).join("/");
    for (const m of result.messages) {
      if (m.severity === 1) warnings.push({ rule: `eslint/${m.ruleId ?? "unknown"}`, file, line: m.line, message: m.message });
    }
  }
  const failing = results.filter((r) => r.errorCount > 0).map((r) => ({ ...r, messages: r.messages.filter((m) => m.severity === 2), warningCount: 0 }));
  const formatter = await eslint.loadFormatter("stylish");
  return { warnings, errors: failing.length > 0 ? await formatter.format(failing) : "" };
}

/** Every `eslint-disable` comment in the linted files, each with whether it says why (`-- why`). */
export function eslintExceptions(root: string, files: readonly string[]): Exception[] {
  const out: Exception[] = [];
  for (const file of files) {
    for (const line of fs.readFileSync(path.join(root, file), "utf8").split(/\r?\n/)) {
      const hit = DISABLE.exec(line);
      if (hit) out.push({ file, text: hit[0].trim(), hasReason: hit[0].includes(" -- ") });
    }
  }
  return out;
}

/** The folders ESLint ignores inside the linted ones (eslint.config.mjs): the generated command
 *  types and the vendored libraries, whose comments are their authors', not exceptions of ours. */
const UNLINTED = new Set(["generated", "vendor"]);

/** The TypeScript and JavaScript files under the linted folders, repository-relative. */
export function lintedFiles(root: string): string[] {
  const out: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of fs.readdirSync(path.join(root, dir), { withFileTypes: true })) {
      const rel = `${dir}/${entry.name}`;
      if (entry.isDirectory() && !UNLINTED.has(entry.name)) walk(rel);
      else if (/\.(m?[jt]sx?)$/.test(entry.name)) out.push(rel);
    }
  };
  linted(fs.readFileSync(path.join(root, "package.json"), "utf8")).forEach(walk);
  return out.sort();
}
