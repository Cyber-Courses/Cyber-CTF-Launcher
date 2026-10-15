// The Tauri commands a set of pages can invoke, found statically: walk the imports from the
// entry files (through `@/` and relative paths), and collect the commands named by the binding
// wrappers each file imports from src/lib/tauri, plus any direct `invoke("...")`.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const SRC = fileURLToPath(new URL("..", import.meta.url)).replace(/[\\/]$/, "");
const BINDINGS = path.join(SRC, "lib/tauri");
const INVOKERS = new Set(["invoke", "invokeUnit", "invokeStreaming"]);

const parse = (file: string) => ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true);
const isBinding = (file: string) => file.startsWith(BINDINGS + path.sep) && !file.includes(".test.");

/** The source file an import specifier names, or null for a package. */
function resolve(from: string, spec: string): string | null {
  const base = spec.startsWith("@/") ? path.join(SRC, spec.slice(2)) : spec.startsWith(".") ? path.resolve(path.dirname(from), spec) : null;
  if (!base) return null;
  const candidates = [base, `${base}.ts`, `${base}.tsx`, path.join(base, "index.ts"), path.join(base, "index.tsx")];
  return candidates.find((c) => fs.existsSync(c) && fs.statSync(c).isFile()) ?? null;
}

/** The command names passed as a string literal to `invoke`/`invokeStreaming` under `node`. */
function invokedIn(node: ts.Node, out = new Set<string>()): Set<string> {
  if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && INVOKERS.has(node.expression.text)) {
    const first = node.arguments.at(0);
    if (first && ts.isStringLiteralLike(first)) out.add(first.text);
  }
  ts.forEachChild(node, (child) => void invokedIn(child, out));
  return out;
}

/** The name an exported top-level binding declares, with the node holding its body. */
function declared(node: ts.Node): [string, ts.Node][] {
  if (ts.isFunctionDeclaration(node) && node.name) return [[node.name.text, node]];
  if (ts.isVariableStatement(node)) return node.declarationList.declarations.map((d) => [d.name.getText(), d]);
  return [];
}

/** Each binding wrapper's name and the commands it invokes. */
function bindingCommands(): Map<string, Set<string>> {
  const out = new Map<string, Set<string>>();
  for (const name of fs.readdirSync(BINDINGS)) {
    const file = path.join(BINDINGS, name);
    if (!isBinding(file) || !/\.tsx?$/.test(name)) continue;
    parse(file).forEachChild((node) => {
      declared(node).forEach(([fn, body]) => out.set(fn, invokedIn(body)));
    });
  }
  return out;
}

/** The names an import declaration takes from a module (every binding for `* as`). */
function importedNames(node: ts.ImportDeclaration | ts.ExportDeclaration, bindings: Map<string, Set<string>>): string[] {
  const named = ts.isImportDeclaration(node) ? node.importClause?.namedBindings : node.exportClause;
  if (!named) return [];
  if (ts.isNamespaceImport(named)) return [...bindings.keys()];
  if (ts.isNamespaceExport(named)) return [...bindings.keys()];
  return named.elements.map((e) => (e.propertyName ?? e.name).text);
}

/** The commands reachable from `entries` (paths relative to src/). */
export function commandsFrom(entries: string[]): Set<string> {
  const bindings = bindingCommands();
  const seen = new Set<string>();
  const found = new Set<string>();
  const walk = (file: string) => {
    if (seen.has(file) || isBinding(file)) return;
    seen.add(file);
    const source = parse(file);
    invokedIn(source).forEach((c) => found.add(c));
    source.forEachChild((node) => {
      if (!(ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) || !node.moduleSpecifier) return;
      const target = resolve(file, (node.moduleSpecifier as ts.StringLiteral).text);
      if (!target) return;
      if (!isBinding(target)) {
        walk(target);
        return;
      }
      importedNames(node, bindings).forEach((n) => bindings.get(n)?.forEach((c) => found.add(c)));
    });
  };
  entries.forEach((e) => {
    walk(path.join(SRC, e));
  });
  return found;
}

/** The commands each permission set in src-tauri/permissions/windows.toml allows. */
export function permissionSets(): Map<string, Set<string>> {
  const toml = fs.readFileSync(path.join(SRC, "../src-tauri/permissions/windows.toml"), "utf8");
  const out = new Map<string, Set<string>>();
  for (const block of toml.split("[[set]]").slice(1)) {
    const id = /identifier = "([^"]+)"/.exec(block)?.[1] ?? "";
    const allowed = [...block.matchAll(/"allow-([a-z0-9-]+)"/g)].map((m) => m[1].replaceAll("-", "_"));
    out.set(id, new Set(allowed));
  }
  return out;
}
