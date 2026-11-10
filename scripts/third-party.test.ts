import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { cdxLicense, licenseFiles, npmNotice, npmSbomOf, purl, type NpmLicensed, type NpmNode } from "./third-party.ts";

const dirs: string[] = [];
/** A package folder holding `files` (name: text), removed after the test. */
function packageDir(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "third-party-"));
  dirs.push(dir);
  mkdirSync(dir, { recursive: true });
  for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, name), text);
  return dir;
}
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

describe("third-party: identifiers", () => {
  it("names an npm package by its package URL, a scope's @ escaped", () => {
    expect(purl("react", "19.2.8")).toBe("pkg:npm/react@19.2.8");
    expect(purl("@tauri-apps/api", "2.12.0")).toBe("pkg:npm/%40tauri-apps/api@2.12.0");
  });

  it("gives CycloneDX an SPDX expression, an SPDX id, or a free-text licence name", () => {
    expect(cdxLicense("MIT OR Apache-2.0")).toEqual({ expression: "MIT OR Apache-2.0" });
    expect(cdxLicense("Apache-2.0 WITH LLVM-exception")).toEqual({ expression: "Apache-2.0 WITH LLVM-exception" });
    expect(cdxLicense("BSD-3-Clause")).toEqual({ license: { id: "BSD-3-Clause" } });
    expect(cdxLicense("Custom: see LICENSE")).toEqual({ license: { name: "Custom: see LICENSE" } });
  });
});

describe("third-party: notices", () => {
  it("takes a package's licence and notice files, sorted, and nothing else", () => {
    const dir = packageDir({ "LICENSE.md": "MIT text\n", NOTICE: "Notice text", "COPYING-2": "copying", "README.md": "no", "licensed.js": "no" });
    expect(licenseFiles(dir)).toEqual(["COPYING-2:\n\ncopying", "LICENSE.md:\n\nMIT text", "NOTICE:\n\nNotice text"]);
    expect(licenseFiles(join(dir, "missing"))).toEqual([]);
  });

  it("writes each licence text once across a package's copies, else what its package.json declares", () => {
    const text = { LICENSE: "The MIT licence" };
    const pkg: NpmLicensed = {
      name: "left-pad",
      versions: ["1.3.0", "1.2.0"],
      paths: [packageDir(text), packageDir(text)],
      license: "MIT",
      homepage: "https://x.test",
    };
    const notice = npmNotice(pkg);
    expect(notice).toContain("left-pad 1.3.0, 1.2.0 (MIT) <https://x.test>");
    expect(notice.split("The MIT licence")).toHaveLength(2);
    const bare = npmNotice({ name: "bare", versions: ["1.0.0"], paths: [packageDir({})], license: "ISC" });
    expect(bare).toContain("bare 1.0.0 (ISC)\n\nThe package ships no licence file; its package.json declares ISC.");
  });
});

describe("third-party: the npm SBOM", () => {
  const leaf: NpmNode = { version: "1.0.0", resolved: "https://registry.npmjs.org/leaf/-/leaf-1.0.0.tgz" };
  // `mid` appears twice: first deduped (no children), then with them, as pnpm may list it.
  const root = {
    name: "cyber-ctf-desktop",
    version: "1.8.9",
    dependencies: {
      a: { version: "2.0.0", dependencies: { mid: { version: "3.0.0" } } },
      b: { version: "2.1.0", dependencies: { mid: { version: "3.0.0", dependencies: { leaf } } } },
      unresolved: {},
    },
    optionalDependencies: { "@scope/opt": { version: "0.1.0" } },
  };
  const packages: NpmLicensed[] = [
    { name: "a", versions: ["2.0.0"], paths: [], license: "MIT" },
    { name: "leaf", versions: ["1.0.0"], paths: [], license: "MIT OR Apache-2.0" },
  ];
  type Sbom = {
    metadata: { timestamp: string; component: { "bom-ref": string } };
    components: { "bom-ref": string; licenses?: unknown[]; externalReferences?: unknown[] }[];
    dependencies: { ref: string; dependsOn: string[] }[];
  };
  const sbom = npmSbomOf(root, packages, "2026-01-01T00:00:00.000Z") as Sbom;

  it("lists every resolved package once, sorted, with its licence and where it was fetched", () => {
    expect(sbom.components.map((c) => c["bom-ref"])).toEqual([
      "pkg:npm/%40scope/opt@0.1.0",
      "pkg:npm/a@2.0.0",
      "pkg:npm/b@2.1.0",
      "pkg:npm/leaf@1.0.0",
      "pkg:npm/mid@3.0.0",
    ]);
    const leafOf = sbom.components.find((c) => c["bom-ref"] === "pkg:npm/leaf@1.0.0");
    expect(leafOf?.licenses).toEqual([{ expression: "MIT OR Apache-2.0" }]);
    expect(leafOf?.externalReferences).toEqual([{ type: "distribution", url: leaf.resolved }]);
    expect(sbom.components.find((c) => c["bom-ref"] === "pkg:npm/b@2.1.0")?.licenses).toBeUndefined();
  });

  it("keeps each package's edges, a deduped one's from where pnpm lists its children", () => {
    expect(sbom.metadata).toMatchObject({ timestamp: "2026-01-01T00:00:00.000Z", component: { "bom-ref": "pkg:npm/cyber-ctf-desktop@1.8.9" } });
    expect(sbom.dependencies).toEqual([
      { ref: "pkg:npm/a@2.0.0", dependsOn: ["pkg:npm/mid@3.0.0"] },
      { ref: "pkg:npm/b@2.1.0", dependsOn: ["pkg:npm/mid@3.0.0"] },
      { ref: "pkg:npm/cyber-ctf-desktop@1.8.9", dependsOn: ["pkg:npm/%40scope/opt@0.1.0", "pkg:npm/a@2.0.0", "pkg:npm/b@2.1.0"] },
      { ref: "pkg:npm/mid@3.0.0", dependsOn: ["pkg:npm/leaf@1.0.0"] },
    ]);
  });
});
