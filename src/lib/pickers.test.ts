// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { pickFile, pickFolder } from "@/lib/pickers";
import { WDIO_PICK } from "@/lib/wdio";
import { installTauri } from "@/test/tauri";

afterEach(() => {
  localStorage.removeItem(WDIO_PICK);
});

describe("pickFolder and pickFile", () => {
  it("open the system's picker with the title and the folder they are given, and give back what it picked", async () => {
    let picked: string | null = "/Users/player/loot";
    const calls = installTauri({ "plugin:dialog|open": () => picked });
    expect(await pickFolder({ title: "Pick a folder", defaultPath: "/Users/player" })).toBe("/Users/player/loot");
    expect(calls.at(-1)).toEqual({
      cmd: "plugin:dialog|open",
      args: { options: { title: "Pick a folder", defaultPath: "/Users/player", directory: true, multiple: false } },
    });
    expect(await pickFile({ title: "Pick a key" })).toBe("/Users/player/loot");
    expect(calls.at(-1)?.args).toEqual({ options: { title: "Pick a key", directory: false, multiple: false } });
    // Closed without a choice: null, as the plugin answers, never a path.
    picked = null;
    expect(await pickFolder({ title: "Pick a folder" })).toBeNull();
  });

  it("are answered by the GUI e2e's hook, with no picker opened", async () => {
    // Regression: the specs patched the webview's `invoke` to answer the picker, which Tauri
    // defines read-only, so the real picker opened under the e2e and the spec timed out.
    const calls = installTauri({ "plugin:dialog|open": () => "/never/opened" });
    localStorage.setItem(WDIO_PICK, "/tmp/cyberctf-shared-x");
    expect(await pickFolder({ title: "Pick a folder" })).toBe("/tmp/cyberctf-shared-x");
    expect(await pickFile({ title: "Pick a key" })).toBe("/tmp/cyberctf-shared-x");
    expect(calls.filter((c) => c.cmd === "plugin:dialog|open")).toHaveLength(0);
    localStorage.removeItem(WDIO_PICK);
    expect(await pickFolder({ title: "Pick a folder" })).toBe("/never/opened");
  });
});
