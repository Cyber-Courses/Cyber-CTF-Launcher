// The ratchet's data: what a collector finds, how findings group into counts, and where a file
// belongs. A count is kept per rule and per area (a Rust domain module, a frontend feature), so
// the fixing can be split by area and a violation can't hide behind a fix elsewhere.

/** One violation: the rule it breaks, the repository-relative file (with `/`) and where. */
export type Finding = { rule: string; file: string; line: number; message: string };

/** A local exception: an `#[allow]`/`#[expect]` or an `eslint-disable` line, as written. */
export type Exception = { file: string; text: string; hasReason: boolean };

/** Counts per rule, then per area. */
export type Counts = Record<string, Record<string, number>>;

/** Counts per file, then per exception text: each exception the review accepted. */
export type ExceptionCounts = Record<string, Record<string, number>>;

/** What a run measures. Clippy's findings depend on the host (`cfg(windows)` code), so they are
 *  kept apart and compared with the baseline of the same platform. */
export type Measure = { findings: Finding[]; clippy: Finding[]; exceptions: Exception[] };

/** The area a repository-relative file counts in: `rs:<top module>` for the Rust crate (`lib.rs`,
 *  `main.rs` and `error.rs` are `rs:root`), `ts:features/<feature>` or `ts:<folder>` for the
 *  frontend, and `ts:root` for the package manifest. */
export function areaOf(file: string): string {
  const rust = /^src-tauri\/src\/([^/]+)/.exec(file);
  if (rust) {
    const top = rust[1].replace(/\.rs$/, "");
    return ["lib", "main", "error"].includes(top) ? "rs:root" : `rs:${top}`;
  }
  if (file.startsWith("src-tauri/")) return "rs:root";
  const feature = /^src\/features\/([^/]+)\//.exec(file);
  if (feature) return `ts:features/${feature[1]}`;
  const folder = /^(?:src\/)?([^/]+)\//.exec(file);
  return folder ? `ts:${folder[1]}` : "ts:root";
}

/** Findings counted per rule and area. */
export function countFindings(findings: readonly Finding[]): Counts {
  const counts: Counts = {};
  for (const f of findings) {
    const byArea = (counts[f.rule] ??= {});
    const area = areaOf(f.file);
    byArea[area] = (byArea[area] ?? 0) + 1;
  }
  return counts;
}

/** Exceptions counted per file and text. */
export function countExceptions(exceptions: readonly Exception[]): ExceptionCounts {
  const counts: ExceptionCounts = {};
  for (const e of exceptions) {
    const byText = (counts[e.file] ??= {});
    byText[e.text] = (byText[e.text] ?? 0) + 1;
  }
  return counts;
}

/** `counts` with its keys sorted at both levels, so the baseline file diffs cleanly. */
export function sorted<T>(counts: Record<string, Record<string, T>>): Record<string, Record<string, T>> {
  const out: Record<string, Record<string, T>> = {};
  for (const key of Object.keys(counts).sort()) {
    const inner = counts[key];
    out[key] = Object.fromEntries(
      Object.keys(inner)
        .sort()
        .map((k) => [k, inner[k]]),
    );
  }
  return out;
}

/** The sum of a rule's counts over its areas. */
export const total = (byArea: Record<string, number> | undefined): number => Object.values(byArea ?? {}).reduce((a, b) => a + b, 0);
