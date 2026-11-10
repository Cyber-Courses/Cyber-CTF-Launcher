// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { confirmDestructive } from "@/lib/confirm";
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
