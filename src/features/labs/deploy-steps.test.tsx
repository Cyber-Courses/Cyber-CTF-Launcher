// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DeploySteps } from "@/features/labs/deploy-steps";
import type { DeployStep } from "@/lib/tauri/generated";
import type { SaidStep } from "@/lib/tauri/run-events";

const vagrant = [
  "==> DC01: Importing base box 'StefanScherer/windows_2019'...",
  "==> DC01: Booting VM...",
  "==> SRV02: Importing base box 'StefanScherer/windows_2019'...",
  "==> SRV02: Booting VM...",
];
const at = (n: number, step = 1000) => Array.from({ length: n }, (_, i) => 1_000_000 + i * step);

/** A step the runner said before line `line` (when it came: `times`'), as `id[:label] state[ done/total]`. */
function said(line: number, text: string, times = at(line + 1)): SaidStep {
  const [name, state, count] = text.split(" ");
  const [step, label = null] = name.split(":");
  const parts = count ? count.split("/").map(Number) : [];
  const counted = count ? { done: parts[0], total: parts.length > 1 ? parts[1] : null } : null;
  return { step: { step, state: state as DeployStep["state"], label, count: counted }, at: times.at(line) ?? times.at(-1) ?? 0, line };
}

/** The two machines' starts: DC01's from its first line, SRV02's from its first, 2s apart. */
const machines = [said(0, "up:DC01 started", at(4, 2000)), said(2, "up:DC01 done", at(4, 2000)), said(2, "up:SRV02 started", at(4, 2000))];

const pulse = (name: RegExp) => screen.getByRole("button", { name }).querySelector(".animate-pulse");

describe("DeploySteps", () => {
  it("runs a step the runner said until it says it ended, how far it got beside it", () => {
    const lines = [" Image nginx Pulling", " Image nginx Pulled", " Network web_default Creating"];
    const marks = [said(0, "images started 0/1"), said(2, "images done 1/1"), said(2, "create started 0/2")];
    render(<DeploySteps lines={lines} times={at(3)} marks={marks} busy ready={false} />);
    expect(pulse(/Get images/)).toBeNull();
    expect(screen.getByRole("button", { name: /Get images/ }).textContent).toContain("1/1");
    expect(pulse(/Create the network and containers/)).not.toBeNull();
    expect(screen.getByRole("button", { name: /Create the network and containers/ }).textContent).toContain("0/2");
  });

  it("puts what runs after Ready (the attack box) in a step of its own below Ready, the start's steps all done", () => {
    // Regression: the attack box's image pull went back into the start's "Get images" step,
    // which turned running again under a green Ready.
    const start = [" Image nginx Pulling", " Image nginx Pulled", " Container web Started", "✓ Lab is running"];
    const attack = [" Image kali-attack Pulling", "Adding the attack box…"];
    // The attack box's own steps (its `up` on the running lab) come after Ready: not the start's.
    const marks = [said(0, "images started"), said(2, "images done 1/1"), said(2, "start started"), said(3, "start done 1/1"), said(4, "images started")];
    render(<DeploySteps lines={[...start, ...attack]} times={at(6)} marks={marks} busy ready={false} />);
    expect(pulse(/Get images/)).toBeNull();
    expect(pulse(/Add the attack box/)).not.toBeNull();
    const items = screen.getAllByRole("listitem").map((li) => li.textContent);
    const readyAt = items.findIndex((text) => text.includes("Ready"));
    const attackAt = items.findIndex((text) => text.includes("Add the attack box"));
    expect(readyAt).toBeGreaterThanOrEqual(0);
    expect(attackAt).toBeGreaterThan(readyAt);
  });

  it("shows a Vagrant start as one step per machine with its duration, the running one saying what it does", () => {
    render(<DeploySteps lines={vagrant} times={at(4, 2000)} marks={machines} busy ready={false} where="this machine" />);
    expect(screen.getByText("Starting")).toBeTruthy();
    expect(screen.getByText("DC01")).toBeTruthy();
    expect(screen.getByText("SRV02")).toBeTruthy();
    expect(screen.getByText("this machine")).toBeTruthy();
    // DC01 took until the runner said it was up: 4s.
    expect(screen.getByRole("button", { name: /DC01/ }).textContent).toContain("4.0s");
    expect(screen.getByRole("button", { name: /SRV02/ }).textContent).toContain("Start the machine");
    // The running step shows its latest line inline.
    expect(screen.getByText("==> SRV02: Booting VM...")).toBeTruthy();
  });

  it("shows a running VM upload to ESXi as how far it is", () => {
    const lines = ["==> probe: --- Guest Build ---", "Opening VMX source: /tmp/probe.vmx", "Disk progress: 41%", "Disk progress: 42%"];
    render(<DeploySteps lines={lines} times={at(4)} marks={[said(0, "up:probe started")]} busy ready={false} />);
    expect(screen.getByText("Uploading: 42%")).toBeTruthy();
    expect(screen.queryByText("Disk progress: 42%")).toBeNull();
  });

  it("marks the machine the runner said failed, not the last one shown", () => {
    const lines = [...vagrant, "✗ An error occurred while executing the action on the 'DC01' machine."];
    const marks = [said(0, "up:DC01 started"), said(2, "up:SRV02 started"), said(4, "up:DC01 failed"), said(4, "up:SRV02 skipped")];
    render(<DeploySteps lines={lines} times={at(5)} marks={marks} busy={false} ready={false} />);
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(screen.getByText("DC01").className).toContain("text-destructive");
    expect(screen.getByText("SRV02").className).not.toContain("text-destructive");
    expect(screen.getByText("An error occurred while executing the action on the 'DC01' machine.")).toBeTruthy();
  });

  it("marks the step holding the last line failed when the runner said none failed", () => {
    render(<DeploySteps lines={[...vagrant, "✗ Vagrant crashed"]} times={at(5)} marks={machines} busy={false} ready={false} />);
    expect(screen.getByText("SRV02").className).toContain("text-destructive");
  });

  it("ends a finished launch with Ready, expands a step and the full log", async () => {
    const user = userEvent.setup();
    render(<DeploySteps lines={[...vagrant, "✓ Lab is running"]} times={at(5)} marks={[...machines, said(4, "up:SRV02 done")]} busy={false} ready={false} />);
    expect(screen.getAllByText("Ready").length).toBe(2);
    const step = screen.getByRole("button", { name: /DC01/ });
    await user.click(step);
    expect(step.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("==> DC01: Booting VM...")).toBeTruthy();
    await user.click(step);
    expect(step.getAttribute("aria-expanded")).toBe("false");
    await user.click(screen.getByRole("button", { name: /Show full log/ }));
    expect(screen.getByRole("button", { name: /Hide full log/ })).toBeTruthy();
    expect(document.body.textContent).toContain("✓ Lab is running");
  });

  it("names an operation on a built lab after it, not as a new build", () => {
    const { rerender } = render(<DeploySteps lines={["==> DC01: Saving state"]} times={at(1)} busy ready={false} operation="pause" />);
    expect(screen.getByText("Pausing")).toBeTruthy();
    expect(screen.getByText("Save the machines' state")).toBeTruthy();
    rerender(<DeploySteps lines={["==> DC01: Saving state", "✓ Lab paused"]} times={at(2)} busy={false} ready={false} operation="pause" />);
    expect(screen.getByText("Paused")).toBeTruthy();
    expect(screen.queryByText("Ready")).toBeNull();
    rerender(<DeploySteps lines={[]} times={[]} busy ready={false} operation="shutdown" />);
    expect(screen.getByText("Shutting down…")).toBeTruthy();
    rerender(<DeploySteps lines={["x"]} times={at(1)} busy={false} ready operation="resume" />);
    expect(screen.getByText("Ready")).toBeTruthy();
  });

  it("says it is preparing before the first line of a launch", () => {
    render(<DeploySteps lines={[]} times={[]} busy ready={false} />);
    expect(screen.getByText("Preparing…")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Show full log/ })).toBeNull();
  });

  it("starts over on a new run, and ticks the running step's time on its own", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const creating = [said(0, "create started")];
    const { rerender } = render(<DeploySteps lines={["Container api Creating"]} times={at(1)} marks={creating} busy ready={false} />);
    const starting = [...creating, said(1, "create done 1/1", at(2)), said(1, "start started 0/1", at(2))];
    rerender(<DeploySteps lines={["Container api Creating", "Container api Started"]} times={at(2)} marks={starting} busy ready={false} />);
    expect(screen.getByText("Start the containers")).toBeTruthy();
    // The running step's time and the run's grow with the clock alone.
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(screen.getByRole("button", { name: /Start the containers/ }).textContent).toContain("2.0s");
    rerender(<DeploySteps lines={["Pulling api"]} times={at(1)} marks={[said(0, "images started")]} busy ready={false} />);
    expect(screen.queryByText("Start the containers")).toBeNull();
    expect(screen.getByText("Get images")).toBeTruthy();
    vi.useRealTimers();
  });
});
