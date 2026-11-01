// @vitest-environment jsdom
import { createElement, type ComponentProps } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MachineSetup } from "@/features/machine/machine-setup";
import { Onboarding } from "@/features/onboarding/onboarding";
import { readyReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// A step's body that crashes is kept inside its boundary: the wizard's frame (header, progress,
// Back) stays usable, and Try again renders the step once more.

// The step bodies, with a switch to make every one of them throw while rendering.
const boom = vi.hoisted(() => ({ on: false }));
vi.mock("@/features/machine/setup-steps", async (importOriginal) => {
  const real = await importOriginal<typeof import("@/features/machine/setup-steps")>();
  return {
    ...real,
    MachineStepBody: (props: ComponentProps<typeof real.MachineStepBody>) => {
      if (boom.on) throw new Error("engine check exploded");
      return createElement(real.MachineStepBody, props);
    },
  };
});

const STEP_FAILED = "This step couldn’t load";

beforeEach(() => {
  boom.on = true;
  // React logs the caught error and its component stack: expected here.
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  boom.on = false;
  vi.restoreAllMocks();
});

/** The boundary's panel is up, with the step's own error in it. */
function expectStepFailed() {
  expect(screen.getByRole("heading", { name: STEP_FAILED })).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain("engine check exploded");
}

/** The step renders again on Try again once it no longer throws, and the panel is gone. */
async function tryAgain(user: ReturnType<typeof userEvent.setup>) {
  boom.on = false;
  await user.click(button(/Try again/));
  await flush();
  expect(screen.queryByRole("heading", { name: STEP_FAILED })).toBeNull();
}

describe("a step that throws", () => {
  it("shows the error panel inside the machine setup window, with the frame alive, and Try again recovers", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    expectStepFailed();
    // The window around the step still stands: its progress and its way out.
    expect(document.body.textContent).toMatch(/Step 1 of \d+/);
    expect(button(/^\s*Continue/)).toBeTruthy();
    expect(button(/Copy details/)).toBeTruthy();

    await tryAgain(user);
    expect(document.body.textContent).not.toContain("engine check exploded");
    expect(document.body.textContent).toMatch(/Step 1 of \d+/);
  });

  it("shows the error panel inside the onboarding, with Back still there, and Try again recovers", async () => {
    installTauri({ auth_status: () => ({ loggedIn: false, name: null, email: null }), system_check: () => readyReport() });
    const user = userEvent.setup();
    render(<Onboarding onComplete={vi.fn()} />);
    await flush();
    await user.click(button(/get started/i));
    await user.click(button(/^skip for now$/i));
    await flush();
    expectStepFailed();
    // The onboarding's frame: the progress line and the way back.
    expect(document.body.textContent).toMatch(/3 \/ \d+ · /);
    expect(button(/^\s*Back/).hasAttribute("disabled")).toBe(false);

    await tryAgain(user);
    expect(document.body.textContent).toMatch(/3 \/ \d+ · /);
  });
});
