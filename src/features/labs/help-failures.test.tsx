// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CopyText } from "@/features/labs/network-diagram/copy-text";
import { GuidedHelp } from "@/features/labs/objectives/guided-help";
import type { Challenge, LabProgress } from "@/features/labs/objectives/objectives";
import type { ObjectiveActions } from "@/features/labs/objectives/objectives-panel";
import { Toaster } from "@/components/ui/toaster";
import { toastsShown } from "@/components/test-support";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// Copying an address off the diagram and opening a challenge's help can fail (no clipboard, the
// backend down): each is said, and the panel stays usable.

const challenge: Challenge = {
  key: "read-secret",
  title: "Read the secret",
  prompt: "What is the password?",
  optional: false,
  points: null,
  hintCount: 1,
  steps: [{ key: "service-account", title: "Find the service account", prompt: null, hintCount: 0 }],
};
const fresh: LabProgress = { solved: [], stepsOpened: [], hints: [], requiredSolved: 0, requiredTotal: 1, completed: false };

/** A clipboard whose writes fail. */
function brokenClipboard() {
  Object.defineProperty(navigator, "clipboard", { value: { writeText: () => Promise.reject(new Error("Document is not focused")) }, configurable: true });
}

const actions = (over: Partial<ObjectiveActions> = {}): ObjectiveActions => ({
  submit: vi.fn(() => Promise.resolve({ correct: false, alreadySolved: false, labCompleted: false })),
  showSteps: vi.fn(() => Promise.resolve()),
  revealHint: vi.fn(() => Promise.resolve()),
  ...over,
});

beforeEach(() => {
  toastsShown();
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("CopyText on the diagram", () => {
  it("says the copy failed and keeps the chip, without crashing", async () => {
    installTauri();
    // After `setup`: user-event installs a working clipboard of its own.
    const user = userEvent.setup();
    brokenClipboard();
    render(
      <>
        <CopyText text="10.42.0.3">10.42.0.3</CopyText>
        <Toaster />
      </>,
    );
    await user.click(screen.getByTitle("Copy 10.42.0.3"));
    await flush();
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Couldn't copy to the clipboard");
    expect(alert.textContent).toContain("Document is not focused");
    expect(screen.getByTitle("Copy 10.42.0.3")).toBeTruthy();
  });
});

describe("GuidedHelp", () => {
  it("says why the steps couldn't open and offers the button again", async () => {
    installTauri();
    const user = userEvent.setup();
    const a = actions({ showSteps: vi.fn(() => Promise.reject(new Error("backend unreachable"))) });
    render(<GuidedHelp challenge={challenge} progress={fresh} signedIn actions={a} />);
    await user.click(screen.getByRole("button", { name: /Guided mode/i }));
    await flush();
    expect(a.showSteps).toHaveBeenCalledWith("read-secret");
    expect(screen.getByText("Couldn't open it: backend unreachable")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Guided mode/i }).hasAttribute("disabled")).toBe(false);
    // No steps show: nothing was opened.
    expect(document.body.textContent).not.toContain("Find the service account");
  });

  it("says why a hint couldn't be revealed", async () => {
    installTauri();
    const user = userEvent.setup();
    const a = actions({ revealHint: vi.fn(() => Promise.reject(new Error("You're signed out. Sign in again to continue."))) });
    render(<GuidedHelp challenge={challenge} progress={fresh} signedIn actions={a} />);
    await user.click(screen.getByRole("button", { name: /Reveal hint 1 of 1/i }));
    await flush();
    expect(a.revealHint).toHaveBeenCalledWith("read-secret", 0);
    expect(screen.getByText("Couldn't open it: You're signed out. Sign in again to continue.")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Reveal hint 1 of 1/i }).hasAttribute("disabled")).toBe(false);
  });
});
