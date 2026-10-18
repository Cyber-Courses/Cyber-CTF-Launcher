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
