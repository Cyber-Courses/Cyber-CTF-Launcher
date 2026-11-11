// Guards against the one Tauri failure that only ever shows up at release time: the JS
// packages (@tauri-apps/*) and their Rust crates (tauri, tauri-plugin-*) drifting onto
// different major/minor versions. `tauri build` refuses to build when they disagree, so the
// drift would otherwise surface in release.yml — on a tag, after a long multi-platform build.
//
// This reproduces that check against the *resolved* versions (pnpm-lock.yaml + Cargo.lock, i.e.
// what actually gets installed/compiled, not the loose ^ ranges) in a few milliseconds, so CI
// catches it on every push. Rust-only plugins (e.g. fs, single-instance) and JS packages with no
// matching crate are skipped — there is nothing to compare.
import { readFileSync } from "node:fs";

import { helpAsked } from "../lib/cli.ts";

interface Version {
  major: number;
  minor: number;
  raw: string;
}

function parse(version: string): Version {
  const [major, minor] = version.split(".");
  return { major: Number(major), minor: Number(minor), raw: version };
}

/** The resolved @tauri-apps/{api,cli,plugin-*} versions in a pnpm lockfile's text, by package
 *  (`api`, `plugin-dialog`), the first of each. */
export function jsVersions(lock: string): Map<string, Version> {
  const out = new Map<string, Version>();
  const re = /'@tauri-apps\/(api|cli|plugin-[a-z-]+)@(\d+\.\d+\.\d+)(?:\([^)]*\))?'/g;
  for (const match of lock.matchAll(re)) {
    if (!out.has(match[1])) out.set(match[1], parse(match[2]));
  }
  return out;
}

/** The resolved tauri / tauri-plugin-* crate versions in a Cargo.lock's text, by crate. */
export function rustVersions(lock: string): Map<string, Version> {
  const out = new Map<string, Version>();
  let name: string | null = null;
  for (const line of lock.split("\n")) {
    const nameMatch = /^name = "(tauri|tauri-plugin-[a-z-]+)"$/.exec(line);
    if (nameMatch) {
      name = nameMatch[1];
      continue;
    }
    const versionMatch = /^version = "(\d+\.\d+\.\d+)"$/.exec(line);
    if (versionMatch && name) {
      out.set(name, parse(versionMatch[1]));
      name = null;
    }
  }
  return out;
}

// @tauri-apps/api and @tauri-apps/cli track the `tauri` crate; each plugin tracks its own crate.
function crateFor(jsName: string): string {
  return jsName === "api" || jsName === "cli" ? "tauri" : `tauri-${jsName}`;
}

/** Each JS package with a crate to compare, against it: how many were compared, and a line for
 *  each on another major/minor than its crate. */
export function compare(js: Map<string, Version>, rust: Map<string, Version>): { checked: number; problems: string[] } {
  const problems: string[] = [];
  let checked = 0;
  for (const [jsName, jsVersion] of js) {
    const crate = rust.get(crateFor(jsName));
    if (!crate) continue;
    checked += 1;
    if (jsVersion.major !== crate.major || jsVersion.minor !== crate.minor) {
      problems.push(`  @tauri-apps/${jsName} (${jsVersion.raw})  !=  ${crateFor(jsName)} (${crate.raw})`);
    }
  }
  return { checked, problems };
}

/** The check against the repository's lockfiles: an exit code, and what it says. Comparing
 *  nothing is a failure: a lockfile format the parsers no longer read would otherwise pass. */
export function check(pnpmLock: string, cargoLock: string): { code: number; message: string } {
  const { checked, problems } = compare(jsVersions(pnpmLock), rustVersions(cargoLock));
  if (checked === 0) {
    return {
      code: 1,
      message:
        "No Tauri JS package could be matched with its Rust crate: pnpm-lock.yaml or src-tauri/Cargo.lock\n" +
        "is no longer in the format this check reads. Update scripts/check/tauri-versions.ts.",
    };
  }
  if (problems.length > 0) {
    return {
      code: 1,
      message:
        "Tauri JS packages and Rust crates are on different major/minor versions:\n" +
        problems.join("\n") +
        "\n\nFix: bump the JS package in package.json to the crate's minor (or pin the crate down to the\n" +
        "JS minor) and re-lock, so `tauri build` accepts them. This is the same rule tauri-action enforces\n" +
        "at release time — catching it here avoids a failed release build.",
    };
  }
  return { code: 0, message: `Tauri version consistency OK (${String(checked)} JS package(s) checked against their Rust crates).` };
}

if (process.argv[1]?.endsWith("tauri-versions.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) {
  const { code, message } = check(readFileSync("pnpm-lock.yaml", "utf8"), readFileSync("src-tauri/Cargo.lock", "utf8"));
  (code === 0 ? console.log : console.error)(message);
  process.exit(code);
}
