// The duplicate-code check (jscpd, over the Rust and TypeScript sources) and the unused-code
// check (knip: files, exports and dependencies nothing uses), run from node_modules.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { type Finding, repoRelative } from "./model.ts";

type Clone = { firstFile: { name: string; start: number }; secondFile: { name: string; start: number }; lines: number };
type KnipIssue = { file: string } & Record<string, unknown>;

/** Runs a package's bin script with this Node, from the repository root. */
function runBin(root: string, pkg: string, bin: string, args: readonly string[]): string {
  const script = path.join(root, "node_modules", pkg, bin);
  const run = spawnSync(process.execPath, [script, ...args], { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
  if (run.error) throw run.error;
  return run.stdout;
}

/** Each block of code found twice (at least 50 tokens over 5 lines), in the area of its first copy. */
export function runJscpd(root: string): Finding[] {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "jscpd-"));
  try {
    const args = ["--absolute", "--silent", "--no-tips", "-f", "rust,typescript,tsx", "-i", "**/generated/**", "-r", "json", "-o", out, "src-tauri/src", "src"];
    runBin(root, "jscpd", "run-jscpd.js", args);
    const report = JSON.parse(fs.readFileSync(path.join(out, "jscpd-report.json"), "utf8")) as { duplicates: Clone[] };
    return report.duplicates.map((d) => ({
      rule: "dup/clone",
      file: repoRelative(root, d.firstFile.name),
      line: d.firstFile.start,
      message: `${String(d.lines)} lines also at ${repoRelative(root, d.secondFile.name)}:${String(d.secondFile.start)}`,
    }));
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
}

/** Each unused file, export, type or dependency knip reports, as `knip/<kind>`. */
export function runKnip(root: string): Finding[] {
  const report = JSON.parse(runBin(root, "knip", "bin/knip.js", ["--reporter", "json", "--no-progress", "--no-exit-code"])) as { issues: KnipIssue[] };
  const findings: Finding[] = [];
  for (const issue of report.issues) {
    for (const [kind, items] of Object.entries(issue)) {
      if (!Array.isArray(items)) continue;
      for (const item of items as { name?: string; line?: number }[]) {
        findings.push({ rule: `knip/${kind}`, file: issue.file, line: item.line ?? 1, message: item.name ?? kind });
      }
    }
  }
  return findings;
}
