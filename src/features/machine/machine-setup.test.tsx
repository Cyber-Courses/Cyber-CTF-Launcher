// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MachineSetup } from "@/features/machine/machine-setup";
import { machineSteps } from "@/features/machine/setup-steps";
import { Onboarding } from "@/features/onboarding/onboarding";
import type { SystemReport } from "@/lib/tauri";
import { freshReport, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { must } from "@/components/test-support";
import { installTauri } from "@/test/tauri";
import { clickEverything, flush } from "@/test/ui";

const enabled = () => screen.queryAllByRole("button").filter((b) => !b.hasAttribute("disabled"));
const text = (b: HTMLElement) => b.textContent.trim();

/** Presses the forward button (Get started, Continue, Skip this step) until there is none; the
 *  names of the steps' forward buttons, in order. */
async function walk(user: ReturnType<typeof userEvent.setup>, max = 14) {
  const pressed: string[] = [];
  for (let n = 0; n < max; n++) {
    const forward = enabled().find((b) => /^(get started|continue|next)/i.test(text(b)));
    if (!forward) break;
    pressed.push(text(forward));
    await user.click(forward);
    await flush();
  }
  return pressed;
}

const MACHINES: [string, SystemReport][] = [
  ["a ready Mac", readyReport()],
  ["a fresh Mac", freshReport("macos")],
  ["a fresh Windows PC", freshReport("windows")],
  ["a fresh Linux box", freshReport("linux")],
  ["a Linux box whose Docker is stopped", stoppedDockerReport()],
];

describe("MachineSetup window", () => {
  it("waits for the machine check", () => {
    installTauri();
    render(<MachineSetup report={null} onRefresh={vi.fn()} onClose={vi.fn()} />);
    expect(screen.queryByRole("heading")).toBeNull();
  });

  it("walks a ready machine to the end and closes", async () => {
    installTauri();
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={onClose} />);
    await flush();
    const pressed = await walk(user);
    expect(pressed.length).toBe(machineSteps(readyReport()).length);
    await user.click(screen.getByRole("button", { name: /done|start/i }));
    expect(onClose).toHaveBeenCalled();
    // And back again.
    await user.click(screen.getByRole("button", { name: /back/i }));
    await flush();
  });

  for (const [name, report] of MACHINES) {
    it(`shows what ${name} needs at each step`, async () => {
      const calls = installTauri({ system_check: () => report });
      const user = userEvent.setup();
      const onRefresh = vi.fn();
      for (const [n, step] of machineSteps(report).entries()) {
        const { unmount } = render(<MachineSetup report={report} onRefresh={onRefresh} onClose={vi.fn()} startAt={{ step, nonce: n + 1 }} />);
        await flush();
        await clickEverything(user, { skip: /^(back|continue|done)$/i, max: 10 });
        unmount();
      }
      expect(calls.length).toBeGreaterThan(0);
    });
  }

  it("opens at a requested step", async () => {
    installTauri();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    expect(screen.getAllByText(/6/).length).toBeGreaterThan(0);
  });
});

describe("Onboarding", () => {
  it("walks from welcome to done on a ready machine", async () => {
    installTauri();
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(<Onboarding onComplete={onComplete} />);
    await flush();
    const pressed = await walk(user);
    expect(pressed[0]).toMatch(/get started/i);
    await clickEverything(user, { skip: /skip setup|back/i, max: 10 });
    await flush();
    expect(onComplete).toHaveBeenCalled();
  });

  it("signs in from the sign-in step and reports a failed sign-in", async () => {
    let fail = true;
    installTauri({
      auth_status: () => ({ loggedIn: false, name: null, email: null }),
      auth_login: () => {
        if (fail) {
          fail = false;
          throw new Error("timed out waiting for the browser");
        }
        return { loggedIn: true, name: "Player", email: "p@example.com" };
      },
      system_check: () => freshReport("linux"),
    });
    const user = userEvent.setup();
    render(<Onboarding onComplete={vi.fn()} />);
    await flush();
    await user.click(screen.getByRole("button", { name: /get started/i }));
    await flush();
    const signIn = () =>
      must(
        enabled().find((b) => /sign in|log in/i.test(text(b))),
        "the sign-in button",
      );
    await user.click(signIn());
    await flush();
    expect(screen.getAllByText(/timed out/).length).toBeGreaterThan(0);
    await user.click(signIn());
    await flush();
    expect(screen.getAllByText(/Player/).length).toBeGreaterThan(0);
    await walk(user);
  });

  it("can be skipped", async () => {
    installTauri();
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(<Onboarding onComplete={onComplete} />);
    await user.click(screen.getByRole("button", { name: /skip/i }));
    expect(onComplete).toHaveBeenCalled();
  });
});
