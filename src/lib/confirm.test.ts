// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { confirmDestructive, warnInDialog } from "@/lib/confirm";
import { answersDialog, dialogsAsked, installTauri, rejectWith } from "@/test/tauri";

const question = { title: "Remove AWS sandbox?", message: "Its keys are deleted.", confirm: "Remove account" };

describe("confirmDestructive", () => {
  it("asks in the system dialog, a warning with the action's own button and Cancel", async () => {
    const calls = installTauri();
    expect(await confirmDestructive(question)).toBe(true);
    expect(calls.find((c) => c.cmd === "plugin:dialog|message")?.args).toEqual({
      title: "Remove AWS sandbox?",
      message: "Its keys are deleted.",
      kind: "warning",
      buttons: { OkCancelCustom: ["Remove account", "Cancel"] },
    });
  });

  it("is a no on Cancel, and when the dialog can't open", async () => {
    installTauri({ "plugin:dialog|message": answersDialog(false) });
    expect(await confirmDestructive(question)).toBe(false);
    installTauri({ "plugin:dialog|message": () => rejectWith("no window") });
    expect(await confirmDestructive(question)).toBe(false);
  });

  it("is answered by the GUI e2e's hook, with no dialog opened: confirm is a yes, cancel a no", async () => {
    // Regression: the specs patched the webview's `invoke` to answer the dialog, which Tauri
    // defines read-only, so the real dialog opened under the e2e and nothing could press it.
    const calls = installTauri();
    localStorage.setItem("__wdio_confirm__", "confirm");
    try {
      expect(await confirmDestructive(question)).toBe(true);
      localStorage.setItem("__wdio_confirm__", "cancel");
      expect(await confirmDestructive(question)).toBe(false);
    } finally {
      localStorage.removeItem("__wdio_confirm__");
    }
    expect(dialogsAsked(calls)).toHaveLength(0);
    expect(await confirmDestructive(question)).toBe(true);
    expect(dialogsAsked(calls)).toHaveLength(1);
  });

  it("asks one question at a time: a second press while one is open is a no", async () => {
    let answer: (label: string) => void = () => {};
    const calls = installTauri({ "plugin:dialog|message": () => new Promise<string>((r) => (answer = r)) });
    const first = confirmDestructive(question);
    expect(await confirmDestructive(question)).toBe(false);
    answer("Remove account");
    expect(await first).toBe(true);
    expect(dialogsAsked(calls)).toHaveLength(1);
  });
});

describe("warnInDialog", () => {
  it("says it in the system dialog, a warning with only its Ok, and never throws", async () => {
    const calls = installTauri();
    await warnInDialog({ title: "Some files stayed", message: "/data/runs/x" });
    expect(calls.find((c) => c.cmd === "plugin:dialog|message")?.args).toMatchObject({ title: "Some files stayed", message: "/data/runs/x", kind: "warning" });
    installTauri({ "plugin:dialog|message": () => rejectWith("no window") });
    await expect(warnInDialog({ title: "Some files stayed", message: "/data/runs/x" })).resolves.toBeUndefined();
  });
});
