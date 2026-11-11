import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// tsconfig.json's `include` sweeps every .ts under the repository, so what a `tauri build`
// writes under src-tauri/target (its generated assets, src-tauri/target/<profile>/build/
// cyberctf-desktop-*/out/tauri-codegen-assets/<hash>.ts: binary content with a .ts name) would
// be type-checked by the next `pnpm build` and fail it (TS1127 "Invalid character"). The test rig
// builds twice in one tree (the app with its WebDriver server, then the package), so the folder
// stays excluded.
describe("tsconfig.json", () => {
  it("excludes src-tauri/target, where a tauri build leaves .ts files that aren't TypeScript", () => {
    const config = JSON.parse(readFileSync(new URL("../../tsconfig.json", import.meta.url), "utf8")) as { exclude?: string[] };
    expect(config.exclude).toContain("src-tauri/target");
    expect(config.exclude).toContain("node_modules");
  });
});
