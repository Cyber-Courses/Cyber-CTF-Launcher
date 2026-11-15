// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { DestructivePrompt, WarningPrompt } from "@/components/destructive-prompt";
import { confirmDestructive, warnInDialog } from "@/lib/confirm";

const question = { title: "Remove AWS sandbox?", message: "Its keys are deleted.", confirm: "Remove account" };

afterEach(cleanup);

describe("confirmDestructive", () => {
  it("asks in the app's own dialog, with the action's own button and Cancel", async () => {
    render(<DestructivePrompt />);
    const answer = confirmDestructive(question);
    expect(await screen.findByRole("alertdialog")).toBeTruthy();
    expect(screen.getByText("Remove AWS sandbox?")).toBeTruthy();
    expect(screen.getByText("Its keys are deleted.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
    expect(await answer).toBe(true);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("is a no on Cancel", async () => {
    render(<DestructivePrompt />);
    const answer = confirmDestructive(question);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(await answer).toBe(false);
  });

  it("is answered by the GUI e2e's hook, with no dialog shown: confirm is a yes, cancel a no", async () => {
    render(<DestructivePrompt />);
    localStorage.setItem("__wdio_confirm__", "confirm");
    try {
      expect(await confirmDestructive(question)).toBe(true);
      localStorage.setItem("__wdio_confirm__", "cancel");
      expect(await confirmDestructive(question)).toBe(false);
    } finally {
      localStorage.removeItem("__wdio_confirm__");
    }
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("asks one question at a time: a second press while one is open is a no", async () => {
    render(<DestructivePrompt />);
    const first = confirmDestructive(question);
    expect(await confirmDestructive(question)).toBe(false);
    fireEvent.click(await screen.findByRole("button", { name: "Remove account" }));
    expect(await first).toBe(true);
  });
});

describe("warnInDialog", () => {
  it("says it in the app's own dialog with one button, and resolves once it is closed", async () => {
    render(<WarningPrompt />);
    let closed = false;
    const shown = warnInDialog({ title: "Some files stayed", message: "/data/runs/x" }).then(() => {
      closed = true;
    });
    expect(await screen.findByText("Some files stayed")).toBeTruthy();
    expect(screen.getByText("/data/runs/x")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
    expect(closed).toBe(false);
    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "OK" }));
    });
    await shown;
    expect(closed).toBe(true);
  });
});
