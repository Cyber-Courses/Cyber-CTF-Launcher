// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it } from "vitest";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The launch-confirm event's payload comes from another process: anything that isn't a request
// (a wrong shape, a missing field, a plain string) is dropped, and a proper one opens the dialog.

const request = { sessionId: "s-1", repository: "org/lab", commit: "abcdef1234567890", target: "aws" };

/** Emits `launch-confirm` with `payload` and lets what it causes render. */
async function confirmAsked(payload: unknown) {
  await act(() => emit("launch-confirm", payload));
  await flush();
}

describe("LaunchConfirm", () => {
  it("drops a malformed payload and opens for a proper request", async () => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked("start it");
    await confirmAsked(null);
    await confirmAsked({ sessionId: 1, repository: "org/lab", commit: "abc", target: "aws" });
    await confirmAsked({ sessionId: "s-1", repository: "org/lab", target: "aws" });
    expect(screen.queryByRole("alertdialog")).toBeNull();

    await confirmAsked(request);
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.textContent).toContain("Run this lab on your cloud account?");
    expect(dialog.textContent).toContain("repo: org/lab");
    expect(dialog.textContent).toContain("commit: abcdef123456");
  });

  it("keeps the open request when a malformed payload follows it", async () => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked(request);
    await confirmAsked({ nonsense: true });
    expect(screen.getByRole("alertdialog").textContent).toContain("repo: org/lab");
  });
});
