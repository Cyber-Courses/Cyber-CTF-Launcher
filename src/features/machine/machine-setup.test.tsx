// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { NAVIGATE_EVENT } from "@/lib/app-events";
import { MachineSetup } from "@/features/machine/machine-setup";
import { machineSteps } from "@/features/machine/setup-steps";
import { Onboarding } from "@/features/onboarding/onboarding";
import type { SystemReport } from "@/lib/tauri";
import { setLastTest } from "@/lib/settings";
import { freshReport, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { must } from "@/components/test-support";
import { installTauri } from "@/test/tauri";
import { button, clickEverything, flush } from "@/test/ui";

const enabled = () => screen.queryAllByRole("button").filter((b) => !b.hasAttribute("disabled"));
const text = (b: HTMLElement = document.body) => b.textContent.trim();
/** The path chooser's button on the row named `path` (Set up, or Review once the path is done). */
const pathButton = (path: string) => must(screen.getByText(path).closest("div.flex")?.querySelector("button"), `the ${path} path's button`);

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

  it("walks a ready machine's container path from where it stands to the end, and closes", async () => {
    installTauri();
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={onClose} />);
    await flush();
    // The choice of path first: each lab type, where this machine stands on it.
    expect(screen.getByText("What do you want to run?")).toBeTruthy();
    await user.click(pathButton("Container labs"));
    await flush();
    // Package manager and engine are in: the path opens at the test, which runs on its own.
    expect(text()).toContain("Step 3 of 5");
    expect(text()).toContain("Test container labs");
    const pressed = await walk(user);
    expect(pressed).toEqual(["Continue", "Continue"]);
    expect(text()).toContain("You’re set up");
    await user.click(screen.getByRole("button", { name: /done|start/i }));
    expect(onClose).toHaveBeenCalled();
    // And back again.
    await user.click(screen.getByRole("button", { name: /back/i }));
    await flush();
  });

  it("a reopened setup on a machine with Docker installed and tested starts past the Docker steps", async () => {
    installTauri();
    setLastTest("docker", "ok");
    const user = userEvent.setup();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    // Nothing left on that path: its row says so, and Review goes straight to the summary.
    expect(pathButton("Container labs").textContent).toMatch(/review/i);
    await user.click(pathButton("Container labs"));
    await flush();
    expect(text()).toContain("You’re set up");
    expect(text()).toContain("Tested and ready to run.");
    expect(text()).not.toContain("Container engine");
    // Only the container path's outcome: VM labs were not this walk.
    expect(text()).not.toContain("VM labs");
    // Back still reaches the steps, done as they are.
    await user.click(screen.getByRole("button", { name: /back/i }));
    await flush();
    expect(text()).toContain("Step 4 of 5");
    expect(text()).toContain("Attack machine");
  });

  it("each path lists only its own steps", async () => {
    installTauri();
    const user = userEvent.setup();
    const report = freshReport("linux");
    const { rerender } = render(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    // The container path on a fresh box: the package manager, the engine, its test, the attack box.
    await user.click(pathButton("Container labs"));
    await flush();
    expect(text()).toContain("Step 1 of 5");
    expect(text()).not.toContain("Virtual machines");
    expect(text()).not.toContain("Cloud labs");
    // Its own steps, opened directly.
    rerender(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vm", nonce: 1 }} />);
    expect(text()).toContain("Step 2 of 5");
    expect(text()).toContain("Virtual machines");
    expect(text()).not.toContain("Container engine");
  });

  it("offers the local providers only: cloud accounts go to the Cloud page, and the window closes", async () => {
    installTauri();
    const user = userEvent.setup();
    const onClose = vi.fn();
    // What the main window hears: the request to show a screen.
    const shown: unknown[] = [];
    await listen(NAVIGATE_EVENT, (e) => shown.push(e.payload));
    render(<MachineSetup report={freshReport("macos")} onRefresh={vi.fn()} onClose={onClose} />);
    await flush();
    expect(text()).toContain("Container labs");
    expect(text()).toContain("VM labs");
    expect(text()).toContain("Cloud labs");
    // No path to walk here: the main window shows its Cloud page instead.
    expect(screen.getAllByRole("button", { name: /^set up$/i })).toHaveLength(2);
    await user.click(button(/open the cloud page/i));
    await flush();
    expect(shown).toEqual(["cloud"]);
    expect(onClose).toHaveBeenCalledOnce();
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

  it("opens at a requested step, on its path", async () => {
    installTauri();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Step 3 of 5");
    expect(text()).toContain("Provisioning");
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
