import path from "node:path";
import { describe, expect, it } from "vitest";

import { areaOf, repoRelative } from "./model.ts";

describe("repoRelative", () => {
  it("strips the extended-length prefix jscpd's Windows names carry, so they land in their area", () => {
    const file = repoRelative(String.raw`C:\lw\src`, String.raw`\\?\C:\lw\src\src-tauri\src\platform\exec.rs`, path.win32);
    expect(file).toBe("src-tauri/src/platform/exec.rs");
    expect(areaOf(file)).toBe("rs:platform");
  });

  it("strips the prefix of a file on a Windows share", () => {
    const file = repoRelative(String.raw`\\nas\code\launcher`, String.raw`\\?\UNC\nas\code\launcher\src\features\labs\lab-card.tsx`, path.win32);
    expect(file).toBe("src/features/labs/lab-card.tsx");
    expect(areaOf(file)).toBe("ts:features/labs");
  });

  it("puts a Windows clippy span (the crate's file, with backslashes) in its module's area", () => {
    const file = repoRelative(
      String.raw`C:\lw\src`,
      path.win32.resolve(String.raw`C:\lw\src`, "src-tauri", String.raw`src\deploy_worker\process.rs`),
      path.win32,
    );
    expect(file).toBe("src-tauri/src/deploy_worker/process.rs");
    expect(areaOf(file)).toBe("rs:deploy_worker");
  });

  it("takes plain Windows paths, absolute or relative to the repository, with either separator", () => {
    expect(repoRelative(String.raw`C:\lw\src`, String.raw`C:\lw\src\src\lib\equal.ts`, path.win32)).toBe("src/lib/equal.ts");
    expect(repoRelative(String.raw`C:\lw\src`, String.raw`src-tauri\src\app.rs`, path.win32)).toBe("src-tauri/src/app.rs");
    expect(repoRelative(String.raw`C:\lw\src`, "src-tauri/src/app.rs", path.win32)).toBe("src-tauri/src/app.rs");
  });

  it("takes POSIX paths as they are", () => {
    expect(repoRelative("/code/launcher", "/code/launcher/src-tauri/src/labs.rs", path.posix)).toBe("src-tauri/src/labs.rs");
    expect(repoRelative("/code/launcher", "src/app/page.tsx", path.posix)).toBe("src/app/page.tsx");
  });

  it("keeps a file outside the repository outside it", () => {
    expect(repoRelative("/code/launcher", "/code/isoloom/crates/core/spec/src/lib.rs", path.posix)).toBe("../isoloom/crates/core/spec/src/lib.rs");
  });
});

describe("areaOf", () => {
  it("puts the crate's files under their top module, its root files and manifest under rs:root", () => {
    expect(areaOf("src-tauri/src/runtime/lab/names.rs")).toBe("rs:runtime");
    expect(areaOf("src-tauri/src/cloud.rs")).toBe("rs:cloud");
    for (const root of ["src-tauri/src/lib.rs", "src-tauri/src/main.rs", "src-tauri/src/error.rs", "src-tauri/build.rs"]) expect(areaOf(root)).toBe("rs:root");
  });

  it("puts the frontend's files under their feature or folder, and a file at the top under ts:root", () => {
    expect(areaOf("src/features/labs/lab-row.tsx")).toBe("ts:features/labs");
    expect(areaOf("src/lib/settings.ts")).toBe("ts:lib");
    expect(areaOf("scripts/quality/main.ts")).toBe("ts:scripts");
    expect(areaOf("package.json")).toBe("ts:root");
  });
});
