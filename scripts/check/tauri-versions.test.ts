import { describe, expect, it } from "vitest";
import { check, compare, jsVersions, rustVersions } from "./tauri-versions.ts";

// Excerpts shaped as the real lockfiles are: pnpm-lock.yaml's `packages:` and `snapshots:` keys
// (a peer suffix on some), and Cargo.lock's `[[package]]` entries.
const PNPM_LOCK = `packages:

  '@tauri-apps/api@2.12.0':
    resolution: {integrity: sha512-x}

  '@tauri-apps/cli@2.12.1':
    resolution: {integrity: sha512-y}

  '@tauri-apps/plugin-dialog@2.8.1':
    resolution: {integrity: sha512-z}

  '@tauri-apps/plugin-opener@2.7.0(typescript@5.9.3)':
    resolution: {integrity: sha512-w}

snapshots:

  '@tauri-apps/api@2.12.0': {}
`;

const CARGO_LOCK = `[[package]]
name = "tauri"
version = "2.12.4"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "tauri-build"
version = "2.6.0"

[[package]]
name = "tauri-plugin-dialog"
version = "2.8.0"

[[package]]
name = "tauri-plugin-fs"
version = "2.5.0"
`;

describe("check/tauri-versions", () => {
  it("reads the resolved @tauri-apps versions from the pnpm lockfile, a peer suffix and all", () => {
    const js = jsVersions(PNPM_LOCK);
    expect([...js].map(([name, v]) => [name, v.raw])).toEqual([
      ["api", "2.12.0"],
      ["cli", "2.12.1"],
      ["plugin-dialog", "2.8.1"],
      ["plugin-opener", "2.7.0"],
    ]);
  });

  it("reads the tauri and tauri-plugin-* crates from Cargo.lock, not tauri-build", () => {
    const rust = rustVersions(CARGO_LOCK);
    expect([...rust].map(([name, v]) => [name, v.raw])).toEqual([
      ["tauri", "2.12.4"],
      ["tauri-plugin-dialog", "2.8.0"],
      ["tauri-plugin-fs", "2.5.0"],
    ]);
  });

  it("compares each package with its crate on major and minor, skipping one without a crate", () => {
    const { checked, problems } = compare(jsVersions(PNPM_LOCK), rustVersions(CARGO_LOCK));
    // api and cli against tauri, plugin-dialog against its crate; plugin-opener has none here.
    expect(checked).toBe(3);
    expect(problems).toEqual([]);
    const drifted = compare(jsVersions(PNPM_LOCK), rustVersions(CARGO_LOCK.replace('"2.8.0"', '"2.9.0"')));
    expect(drifted.problems).toEqual(["  @tauri-apps/plugin-dialog (2.8.1)  !=  tauri-plugin-dialog (2.9.0)"]);
  });

  it("passes matching lockfiles and fails a drift", () => {
    expect(check(PNPM_LOCK, CARGO_LOCK)).toMatchObject({ code: 0, message: expect.stringContaining("3 JS package(s) checked") as string });
    expect(check(PNPM_LOCK, CARGO_LOCK.replace('"2.12.4"', '"2.13.0"')).code).toBe(1);
  });

  it("fails when it compares nothing, as with a lockfile it can no longer read", () => {
    const quoted = PNPM_LOCK.replaceAll("'", '"');
    const result = check(quoted, CARGO_LOCK);
    expect(result.code).toBe(1);
    expect(result.message).toContain("No Tauri JS package could be matched");
    expect(check(PNPM_LOCK, "").code).toBe(1);
  });
});
