// Every relative link in the repository's Markdown resolves: the file or folder it names exists,
// and its `#anchor`, when it has one, is a heading of the Markdown file it points to (GitHub's
// anchor rules). External links (http, mailto) aren't fetched. Runs in CI (`pnpm check:docs`).
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, normalize } from "node:path";

import { helpAsked } from "./lib/cli.ts";

/** GitHub's anchor for a heading: lowercase, punctuation dropped, spaces to hyphens. */
export function slug(heading: string): string {
  return heading
    .trim()
    .toLowerCase()
    .replace(/<[^>]*>/g, "")
    .replace(/[`*_~]/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .replace(/\s/g, "-");
}

/** The anchors of a Markdown file's headings (repeated ones get -1, -2, as on GitHub). */
export function anchors(markdown: string): Set<string> {
  const seen = new Map<string, number>();
  const out = new Set<string>();
  let fenced = false;
  for (const line of markdown.split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
    const m = fenced ? null : /^#{1,6}\s+(.*?)\s*#*\s*$/.exec(line);
    if (!m) continue;
    const base = slug(m[1]);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.add(n === 0 ? base : `${base}-${String(n)}`);
  }
  return out;
}

/** The link targets of a Markdown text: inline links and images, and reference definitions. */
export function links(markdown: string): string[] {
  const text = markdown.replace(/^\s*(```|~~~)[\s\S]*?^\s*\1/gm, "").replace(/`[^`\n]*`/g, "");
  const inline = [...text.matchAll(/!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)].map((m) => m[1]);
  const refs = [...text.matchAll(/^\s*\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+"[^"]*")?\s*$/gm)].map((m) => m[1]);
  return [...inline, ...refs];
}

function markdownFiles(): string[] {
  return execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "*.md"], { encoding: "utf8" })
    .split("\n")
    .filter((f) => f && existsSync(f));
}

const anchorCache = new Map<string, Set<string>>();

function anchorsOf(file: string): Set<string> {
  let a = anchorCache.get(file);
  if (!a) {
    a = anchors(readFileSync(file, "utf8"));
    anchorCache.set(file, a);
  }
  return a;
}

/** Why `link` in `file` is broken, or null when it resolves. */
function problem(file: string, link: string): string | null {
  const [path, anchor] = link.split("#", 2) as [string, string | undefined];
  const target = path ? normalize(join(dirname(file), decodeURIComponent(path))) : file;
  if (!existsSync(target)) return "no such file";
  const isPage = target.endsWith(".md") && statSync(target).isFile();
  if (anchor && isPage && !anchorsOf(target).has(anchor.toLowerCase())) return `no heading #${anchor}`;
  return null;
}

/** The broken links of `file` (its text `markdown`), each with why; a link with a scheme (http,
 *  mailto) isn't followed. */
export function brokenLinks(file: string, markdown: string): { link: string; why: string }[] {
  return links(markdown).flatMap((link) => {
    const why = /^[a-z][a-z0-9+.-]*:/i.test(link) ? null : problem(file, link);
    return why ? [{ link, why }] : [];
  });
}

function check(): number {
  const files = markdownFiles();
  let broken = 0;
  for (const file of files) {
    for (const { link, why } of brokenLinks(file, readFileSync(file, "utf8"))) {
      console.error(`${file}: ${link}: ${why}`);
      broken++;
    }
  }
  console.log(`${String(files.length)} Markdown files checked, ${String(broken)} broken links.`);
  return broken;
}

if (process.argv[1]?.endsWith("check-doc-links.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) process.exit(check() > 0 ? 1 : 0);
