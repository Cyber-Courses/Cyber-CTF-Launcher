// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DeploySteps } from "@/features/labs/deploy-steps";

const vagrant = [
  "==> DC01: Importing base box 'StefanScherer/windows_2019'...",
  "==> DC01: Booting VM...",
  "==> SRV02: Importing base box 'StefanScherer/windows_2019'...",
  "==> SRV02: Booting VM...",
];
const at = (n: number, step = 1000) => Array.from({ length: n }, (_, i) => 1_000_000 + i * step);

describe("DeploySteps", () => {
  it("shows a Vagrant start as one step per machine with its duration, the last one running", () => {
    render(<DeploySteps lines={vagrant} times={at(4, 2000)} busy ready={false} where="this machine" />);
    expect(screen.getByText("Building")).toBeTruthy();
    expect(screen.getByText("DC01")).toBeTruthy();
    expect(screen.getByText("SRV02")).toBeTruthy();
    expect(screen.getByText("this machine")).toBeTruthy();
    // DC01 took until SRV02's first line: 4s.
    expect(screen.getByRole("button", { name: /DC01/ }).textContent).toContain("4.0s");
    // The running step shows its latest line inline.
    expect(screen.getByText("==> SRV02: Booting VM...")).toBeTruthy();
  });

  it("marks the machine a failure names as failed, not the last one shown", () => {
    const lines = [...vagrant, "✗ An error occurred while executing the action on the 'DC01' machine."];
    render(<DeploySteps lines={lines} times={at(5)} busy={false} ready={false} />);
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(screen.getByText("DC01").className).toContain("text-destructive");
    expect(screen.getByText("SRV02").className).not.toContain("text-destructive");
    expect(screen.getByText("An error occurred while executing the action on the 'DC01' machine.")).toBeTruthy();
  });

  it("marks the last step failed when the failure names no machine", () => {
    render(<DeploySteps lines={[...vagrant, "✗ Vagrant crashed"]} times={at(5)} busy={false} ready={false} />);
    expect(screen.getByText("SRV02").className).toContain("text-destructive");
  });

  it("ends a finished launch with Ready, expands a step and the full log", async () => {
    const user = userEvent.setup();
    render(<DeploySteps lines={[...vagrant, "✓ Lab is running"]} times={at(5)} busy={false} ready={false} />);
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

  it("starts over on a new run, and ticks the running step's time on its own", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const { rerender } = render(<DeploySteps lines={["Container api Creating"]} times={at(1)} busy ready={false} />);
    rerender(<DeploySteps lines={["Container api Creating", "Container api Started"]} times={at(2)} busy ready={false} />);
    expect(screen.getByText("Start the containers")).toBeTruthy();
    // The running step's time and the run's grow with the clock alone.
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(screen.getByRole("button", { name: /Start the containers/ }).textContent).toContain("2.0s");
    rerender(<DeploySteps lines={["Pulling api"]} times={at(1)} busy ready={false} />);
    expect(screen.queryByText("Start the containers")).toBeNull();
    expect(screen.getByText("Get images")).toBeTruthy();
    vi.useRealTimers();
  });
});
