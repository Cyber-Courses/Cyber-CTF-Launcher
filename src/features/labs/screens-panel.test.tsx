// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { LabMachine } from "@/lib/tauri";
import { ScreensPanel, screenMachines, screenOpens } from "@/features/labs/screens-panel";

// The lab page's screens: a Screen button for each of the lab's machines whose screen opens now,
// why one has none, and nothing for a container or Isoloom's own machines.

const machine = (name: string, state: string, console: LabMachine["console"], infra = false): LabMachine => ({
  name,
  state,
  image: "",
  ip: "",
  ports: [],
  interfaces: [],
  services: [],
  infra,
  console,
});

const MACHINES = [
  machine("dc01", "running", { protocol: "vnc", note: null }),
  machine("ws01", "running", { protocol: "rdp", note: null }),
  machine("web", "running", { protocol: "none", note: "UTM shows a VM's screen over SPICE only" }),
  machine("db", "stopped", { protocol: "vnc", note: null }),
  machine("isoloom-controller", "running", { protocol: "vnc", note: null }, true),
  machine("redis", "running", null),
];

describe("the screens panel", () => {
  it("lists the lab's own machines that say anything of their screen", () => {
    expect(screenMachines(MACHINES).map((m) => m.name)).toEqual(["dc01", "ws01", "web", "db"]);
    expect(MACHINES.filter(screenOpens).map((m) => m.name)).toEqual(["dc01", "ws01", "isoloom-controller"]);
  });

  it("offers a Screen for a running machine that has one, and says why the others don't", async () => {
    const onOpen = vi.fn();
    const user = userEvent.setup();
    render(<ScreensPanel machines={MACHINES} onOpen={onOpen} />);
    expect(screen.getAllByRole("button", { name: /screen/i }).map((b) => b.getAttribute("aria-label"))).toEqual(["Open dc01's screen", "Open ws01's screen"]);
    expect(screen.getByText("No screen: UTM shows a VM's screen over SPICE only")).toBeTruthy();
    expect(screen.getByText("Its screen opens once it runs.")).toBeTruthy();
    expect(screen.getByText("Its Remote Desktop, in your Remote Desktop app")).toBeTruthy();
    expect(screen.queryByText("redis")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Open ws01's screen" }));
    expect(onOpen).toHaveBeenCalledWith("ws01");
  });

  it("shows nothing for a lab with no screen to offer, and why a window didn't open", () => {
    const { container } = render(<ScreensPanel machines={[machine("redis", "running", null)]} onOpen={() => {}} />);
    expect(container.textContent).toBe("");
    render(<ScreensPanel machines={MACHINES} onOpen={() => {}} error="could not open the screen window" />);
    expect(screen.getByText("could not open the screen window")).toBeTruthy();
  });
});
