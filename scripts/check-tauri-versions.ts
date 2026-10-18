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

interface Version {
  major: number;
  minor: number;
  raw: string;
}

function parse(version: string): Version {
  const [major, minor] = version.split(".");
  return { major: Number(major), minor: Number(minor), raw: version };
}

// Resolved @tauri-apps/{api,cli,plugin-*} versions from the pnpm lockfile.
function jsVersions(): Map<string, Version> {
  const lock = readFileSync("pnpm-lock.yaml", "utf8");
  const out = new Map<string, Version>();
  const re = /'@tauri-apps\/(api|cli|plugin-[a-z-]+)@(\d+\.\d+\.\d+)(?:\([^)]*\))?'/g;
  for (const match of lock.matchAll(re)) {
    if (!out.has(match[1])) out.set(match[1], parse(match[2]));
  }
  return out;
}

// Resolved tauri / tauri-plugin-* crate versions from Cargo.lock.
function rustVersions(): Map<string, Version> {
  const lock = readFileSync("src-tauri/Cargo.lock", "utf8");
  const out = new Map<string, Version>();
  let name: string | null = null;
  for (const line of lock.split("\n")) {
    const nameMatch = line.match(/^name = "(tauri|tauri-plugin-[a-z-]+)"$/);
    if (nameMatch) {
      name = nameMatch[1];
      continue;
    }
    const versionMatch = line.match(/^version = "(\d+\.\d+\.\d+)"$/);
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

const js = jsVersions();
const rust = rustVersions();
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

if (problems.length > 0) {
  console.error(
    "Tauri JS packages and Rust crates are on different major/minor versions:\n" +
      problems.join("\n") +
      "\n\nFix: bump the JS package in package.json to the crate's minor (or pin the crate down to the\n" +
      "JS minor) and re-lock, so `tauri build` accepts them. This is the same rule tauri-action enforces\n" +
      "at release time — catching it here avoids a failed release build.",
  );
  process.exit(1);
}

console.log(`Tauri version consistency OK (${String(checked)} JS package(s) checked against their Rust crates).`);
