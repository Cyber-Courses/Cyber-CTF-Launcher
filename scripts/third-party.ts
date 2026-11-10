// What a release says about the third-party software in it, reproducible from a checkout.
//
//   pnpm notices [file]  THIRD_PARTY_NOTICES (default: ./THIRD_PARTY_NOTICES): the licence texts
//                        of the Rust crates compiled into the app (cargo-about) and of the npm
//                        packages in the production dependency tree (pnpm).
//   pnpm sbom [dir]      CycloneDX 1.5 SBOMs (default: ./sbom/): cyberctf-rust.cdx.json
//                        (cargo-cyclonedx) and cyberctf-npm.cdx.json (pnpm's resolved tree).
//
// Needs `pnpm install` done, and on PATH: cargo-about (`cargo install --locked cargo-about
// --features cli`) for notices, cargo-cyclonedx (`cargo install --locked cargo-cyclonedx`) for
// sbom. release.yml runs both on Linux and ships the results with the installers. Set
// SOURCE_DATE_EPOCH to make the SBOMs' timestamps reproducible.
//
// The npm list is every package in the production dependency tree: a superset of what the static
// frontend bundles (Next.js's server-only packages, such as its image library, are listed though
// the app doesn't ship them).
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { helpAsked } from "./lib/cli.ts";

const MANIFEST = "src-tauri/Cargo.toml";
const ABOUT_CONFIG = "scripts/third-party/about.toml";
const ABOUT_TEMPLATE = "scripts/third-party/about.hbs";
const RULE = "-".repeat(80);

/** One npm package as `pnpm licenses list --json` reports it. */
export interface NpmLicensed {
  name: string;
  versions: string[];
  paths: string[];
  license: string;
  homepage?: string;
}

/** A node of `pnpm list --json`'s tree. */
export interface NpmNode {
  version?: string;
  resolved?: string;
  dependencies?: Record<string, NpmNode>;
  optionalDependencies?: Record<string, NpmNode>;
}

function run(program: string, args: string[]): string {
  return execFileSync(program, args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] });
}

function appVersion(): string {
  const pkg = JSON.parse(readFileSync("package.json", "utf8")) as { version: string };
  return pkg.version;
}

/** Every production npm package with its licence, sorted by name. */
function npmPackages(): NpmLicensed[] {
  const byLicense = JSON.parse(run("pnpm", ["licenses", "list", "--prod", "--json"])) as Record<string, NpmLicensed[]>;
  return Object.values(byLicense)
    .flat()
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The licence and notice files a package ships, as `name: text`. */
export function licenseFiles(dir: string): string[] {
  let names: string[];
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  return names
    .filter((n) => /^(licen[cs]e|copying|notice)([.-].*)?$/i.test(n))
    .sort()
    .map((n) => `${n}:\n\n${readFileSync(join(dir, n), "utf8").trimEnd()}`);
}

/** A package's section of the notices: its name, versions, licence and homepage, then the licence
 *  texts its copies ship (each text once), or what its package.json declares when none does. */
export function npmNotice(p: NpmLicensed): string {
  const head = `${p.name} ${p.versions.join(", ")} (${p.license})${p.homepage ? ` <${p.homepage}>` : ""}`;
  const texts = [...new Set(p.paths.flatMap(licenseFiles))];
  const body = texts.length > 0 ? texts.join("\n\n") : `The package ships no licence file; its package.json declares ${p.license}.`;
  return `${RULE}\n${head}\n\n${body}\n`;
}

function notices(file: string): void {
  const rust = run("cargo", ["about", "generate", "--manifest-path", MANIFEST, "-c", ABOUT_CONFIG, ABOUT_TEMPLATE]);
  const npm = npmPackages().map(npmNotice).join("\n");
  const text = [
    `THIRD-PARTY SOFTWARE NOTICES: Cyber CTF ${appVersion()}`,
    "",
    "Cyber CTF is licensed under the Apache License 2.0 (LICENSE, NOTICE). It includes the",
    "third-party software below, under the licences given with each.",
    "",
    "=".repeat(80),
    "Rust crates",
    "=".repeat(80),
    rust.trim(),
    "",
    "=".repeat(80),
    "npm packages (the production dependency tree)",
    "=".repeat(80),
    npm,
  ].join("\n");
  writeFileSync(file, text);
  console.log(`Wrote ${file}`);
}

/** A package URL (purl) for an npm package: `pkg:npm/%40scope/name@version`. */
export function purl(name: string, version: string): string {
  return `pkg:npm/${name.replace(/^@/, "%40")}@${version}`;
}

/** A licence as CycloneDX takes it: an SPDX expression, an SPDX id, or a free-text name. */
export function cdxLicense(license: string): Record<string, unknown> {
  if (/ (OR|AND|WITH) /.test(license)) return { expression: license };
  if (/^[A-Za-z0-9.+-]+$/.test(license)) return { license: { id: license } };
  return { license: { name: license } };
}

/** The npm SBOM of this checkout: its resolved production tree, with the packages' licences. */
function npmSbom(timestamp: string): unknown {
  const [root] = JSON.parse(run("pnpm", ["list", "--prod", "--depth", "Infinity", "--json"])) as (NpmNode & { name: string })[];
  return npmSbomOf(root, npmPackages(), timestamp);
}

/** The npm SBOM of the tree under `root` (`pnpm list`'s): every package once, with its licence
 *  (`packages`, `pnpm licenses list`'s) and what it depends on. */
export function npmSbomOf(root: NpmNode & { name: string }, packages: NpmLicensed[], timestamp: string): unknown {
  const licenses = new Map<string, string>();
  for (const p of packages) for (const v of p.versions) licenses.set(`${p.name}@${v}`, p.license);
  const components = new Map<string, Record<string, unknown>>();
  const edges = new Map<string, Set<string>>();
  const expanded = new Set<string>();
  const rootRef = purl(root.name, root.version ?? "0.0.0");
  const children = (node: NpmNode) => Object.entries({ ...node.dependencies, ...node.optionalDependencies });
  const walk = (parent: string, node: NpmNode) => {
    for (const [name, child] of children(node)) {
      if (!child.version) continue;
      const ref = purl(name, child.version);
      const deps = edges.get(parent) ?? new Set<string>();
      deps.add(ref);
      edges.set(parent, deps);
      if (!components.has(ref)) {
        const license = licenses.get(`${name}@${child.version}`);
        components.set(ref, {
          type: "library",
          "bom-ref": ref,
          name,
          version: child.version,
          purl: ref,
          ...(license ? { licenses: [cdxLicense(license)] } : {}),
          ...(child.resolved ? { externalReferences: [{ type: "distribution", url: child.resolved }] } : {}),
        });
      }
      // pnpm lists a package's dependencies once and marks its other appearances `deduped`
      // (without children), which may come first.
      if (children(child).length > 0 && !expanded.has(ref)) {
        expanded.add(ref);
        walk(ref, child);
      }
    }
  };
  walk(rootRef, root);
  return {
    bomFormat: "CycloneDX",
    specVersion: "1.5",
    version: 1,
    metadata: {
      timestamp,
      tools: { components: [{ type: "application", name: "scripts/third-party.ts" }] },
      component: { type: "application", "bom-ref": rootRef, name: root.name, version: root.version, licenses: [cdxLicense("Apache-2.0")] },
    },
    components: [...components.values()].sort((a, b) => String(a["bom-ref"]).localeCompare(String(b["bom-ref"]))),
    dependencies: [...edges.entries()].map(([ref, deps]) => ({ ref, dependsOn: [...deps].sort() })).sort((a, b) => a.ref.localeCompare(b.ref)),
  };
}

function sbom(dir: string): void {
  mkdirSync(dir, { recursive: true });
  // Every target the launcher ships for, runtime dependencies only.
  run("cargo", [
    "cyclonedx",
    "--manifest-path",
    MANIFEST,
    "-f",
    "json",
    "--spec-version",
    "1.5",
    "--target",
    "all",
    "--no-build-deps",
    "--override-filename",
    "cyberctf-rust.cdx",
    "-q",
  ]);
  renameSync("src-tauri/cyberctf-rust.cdx.json", join(dir, "cyberctf-rust.cdx.json"));
  writeFileSync(join(dir, "cyberctf-npm.cdx.json"), `${JSON.stringify(npmSbom(sbomTimestamp(process.env.SOURCE_DATE_EPOCH, new Date())), null, 2)}\n`);
  console.log(`Wrote ${join(dir, "cyberctf-rust.cdx.json")} and ${join(dir, "cyberctf-npm.cdx.json")}`);
}

/** The SBOM's timestamp: `SOURCE_DATE_EPOCH` (seconds) when set, for a reproducible one, else `now`. */
export function sbomTimestamp(epoch: string | undefined, now: Date): string {
  return (epoch ? new Date(Number(epoch) * 1000) : now).toISOString();
}

/** What the arguments ask for: notices into a file or SBOMs into a folder (each with its default),
 *  or null for anything else, an output that looks like an option (`--help`, a typo) among it. */
export function parseArgs(args: readonly string[]): { command: "notices" | "sbom"; out: string } | null {
  const [command, out] = [args.at(0), args.at(1)];
  if (args.length > 2 || out?.startsWith("-")) return null;
  if (command === "notices") return { command, out: out ?? "THIRD_PARTY_NOTICES" };
  if (command === "sbom") return { command, out: out ?? "sbom" };
  return null;
}

if (process.argv[1]?.endsWith("third-party.ts") && !helpAsked(import.meta.url, process.argv.slice(2))) {
  const asked = parseArgs(process.argv.slice(2));
  if (asked?.command === "notices") notices(asked.out);
  else if (asked?.command === "sbom") sbom(asked.out);
  else {
    console.error("usage: third-party.ts notices [file] | sbom [dir] (--help says more)");
    process.exit(2);
  }
}
