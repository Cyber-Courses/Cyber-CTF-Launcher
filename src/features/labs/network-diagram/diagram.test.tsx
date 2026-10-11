// @vitest-environment jsdom
import { act, fireEvent, render, screen } from "@testing-library/react";
import { Position, ReactFlowProvider } from "@xyflow/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { edgeTypes, link } from "@/features/labs/network-diagram/edges";
import { NetworkDiagram } from "@/features/labs/network-diagram";
import { GREY, type Machine } from "@/features/labs/network-diagram/model";
import type { LabNetwork } from "@/lib/tauri";
import { flush } from "@/test/ui";

// jsdom never measures anything, so React Flow never reports its nodes initialised: pretend it
// did, so the layout pass (ELK, then the frame) runs as in the app.
const initialised = vi.hoisted(() => ({ value: true }));
vi.mock("@xyflow/react", async (orig) => {
  const mod = await orig<typeof import("@xyflow/react")>();
  return { ...mod, useNodesInitialized: () => initialised.value };
});

const machine = (name: string, over: Partial<Machine> = {}): Machine => ({
  name,
  state: "running",
  image: `${name}:latest`,
  ip: "10.42.0.3",
  ports: [],
  interfaces: [{ network: "default", ip: "10.42.0.3" }],
  services: [],
  infra: false,
  ...over,
});

const networks: LabNetwork[] = [{ name: "default", subnet: "10.42.0.0/24", internal: false }];

// Each observed element reports this width: the shell sizes the drawing to it.
let shellWidth = 900;
class SizedObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(el: Element) {
    this.cb([{ target: el, contentRect: { width: shellWidth, height: 400 } } as unknown as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  initialised.value = true;
  shellWidth = 900;
  vi.stubGlobal("ResizeObserver", SizedObserver);
  // React Flow reads the viewport's zoom from the transform when it measures a node.
  vi.stubGlobal(
    "DOMMatrixReadOnly",
    class {
      m22 = 1;
    },
  );
});
afterEach(() => vi.unstubAllGlobals());

const settle = async () => {
  // ELK lays out asynchronously (a fake worker): give it a few turns.
  for (let i = 0; i < 10; i++) await flush(20);
};

describe("NetworkDiagram", () => {
  it("shows your machine, the lab's machines and their services", async () => {
    render(
      <NetworkDiagram
        machines={[
          machine("api", {
            image: "node:20",
            ports: [{ published: 8080, target: 80 }],
            services: [
              { name: "invoice-api", kind: "web", ports: [80] },
              { name: "admin", kind: "web", ports: [9090] },
            ],
          }),
          machine("cache", { image: "redis:7", ip: "10.42.0.5", interfaces: [{ network: "default", ip: "10.42.0.5" }] }),
        ]}
        networks={networks}
      />,
    );
    await settle();
    expect(screen.getByText("Your machine")).toBeTruthy();
    expect(screen.getAllByText("127.0.0.1").length).toBeGreaterThan(0);
    expect(screen.getByText("invoice-api")).toBeTruthy();
    expect(screen.getByText("admin")).toBeTruthy();
    // With declared services, the image moves up onto the card.
    expect(screen.getByText("node:20")).toBeTruthy();
    // A machine without declared services: one row named after its image, no ports.
    expect(screen.getAllByText("redis:7").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/no ports/i).length).toBeGreaterThan(0);
    // The published port docks as a tab on the card's bottom edge, once laid out.
    expect(document.querySelector(".hostcard-docked")).not.toBeNull();
    expect(await screen.findByText(":8080")).toBeTruthy();
    expect(document.querySelector(".port-tab")?.getAttribute("title")).toContain("http://127.0.0.1:8080");
  });

  it("names a server instead of your machine for a remote lab", async () => {
    render(<NetworkDiagram machines={[machine("dc01", { ports: [] })]} host="homelab-pve" />);
    await settle();
    expect(screen.getByText("homelab-pve")).toBeTruthy();
    expect(screen.queryByText("Your machine")).toBeNull();
    expect(document.querySelector(".hostcard-docked")).toBeNull();
    expect(document.querySelector(".port-tab")).toBeNull();
  });

  it("shows each machine's state in words", async () => {
    render(
      <NetworkDiagram
        machines={[
          machine("a", { state: "saved" }),
          machine("b", { state: "poweroff" }),
          machine("c", { state: "" }),
          machine("d", { state: "restarting" }),
          machine("e", { ip: "", interfaces: [{ network: "default", ip: "" }] }),
        ]}
      />,
    );
    await settle();
    const states = Array.from(document.querySelectorAll(".running")).map((n) => n.textContent?.trim());
    expect(states).toEqual(["paused", "off", "stopped", "restarting", "running"]);
    // No address yet: says so instead of a blank.
    expect(screen.getByText(/resolving/i)).toBeTruthy();
  });

  it("draws the attack box and a pivot with an address per network", async () => {
    render(
      <NetworkDiagram
        machines={[
          machine("jump", {
            interfaces: [
              { network: "default", ip: "10.42.0.7" },
              { network: "inner", ip: "10.9.0.7" },
            ],
          }),
        ]}
        networks={[...networks, { name: "inner", subnet: "10.9.0.0/24", internal: true }]}
        attacker={{ running: true, ip: "10.42.0.9", labNetwork: "default" }}
      />,
    );
    await settle();
    expect(screen.getByText("10.42.0.9")).toBeTruthy();
    expect(screen.getByText("10.42.0.7")).toBeTruthy();
    expect(screen.getByText("10.9.0.7")).toBeTruthy();
    expect(screen.getByText(/no internet/i)).toBeTruthy();
    expect(document.querySelectorAll(".iface")).toHaveLength(2);
  });

  it("copies an address on click, and says when it can't", async () => {
    // Clicked without a mouse-down: React Flow's drag handler wants a real view (jsdom has none).
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<NetworkDiagram machines={[machine("api")]} />);
    await settle();
    fireEvent.click(screen.getByText("10.42.0.3"));
    await flush();
    expect(writeText).toHaveBeenCalledWith("10.42.0.3");
    expect(document.querySelector(".copy-ok")).not.toBeNull();

    writeText.mockRejectedValueOnce(new Error("denied"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    fireEvent.click(screen.getAllByText("127.0.0.1")[0]);
    await flush();
    expect(warn).toHaveBeenCalledWith(expect.stringMatching(/copy/i), expect.anything());
  });

  it("does not rewrite the networks it is given", async () => {
    const nets: LabNetwork[] = [{ name: "default", subnet: "", internal: false }];
    render(<NetworkDiagram machines={[machine("api")]} networks={nets} />);
    await settle();
    expect(nets[0].subnet).toBe("");
    expect(screen.getByText("10.42.0.0/16")).toBeTruthy();
  });

  it("keeps the diagram drawn when the layout fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const layout = await import("@/features/labs/network-diagram/layout");
    const spy = vi.spyOn(layout, "layout").mockRejectedValueOnce(new Error("elk crashed"));
    render(<NetworkDiagram machines={[machine("api")]} />);
    await settle();
    spy.mockRestore();
    expect(screen.getByText("api")).toBeTruthy();
    expect(warn).toHaveBeenCalledWith("network diagram layout failed", expect.any(Error));
  });

  it("waits for React Flow's measurements before laying out", async () => {
    initialised.value = false;
    render(<NetworkDiagram machines={[machine("api", { ports: [{ published: 3000, target: 3000 }] })]} />);
    await settle();
    // Not laid out: no tabs yet.
    expect(document.querySelector(".port-tab")).toBeNull();
  });

  it("re-lays out on a scroll of a wide lab", async () => {
    shellWidth = 300;
    render(<NetworkDiagram machines={["a", "b", "c", "d"].map((n, i) => machine(n, { ports: [{ published: 8000 + i, target: 80 }] }))} />);
    await settle();
    const scroller = document.querySelector<HTMLElement>(".topology-scroll")!;
    act(() => {
      scroller.scrollLeft = 120;
      scroller.dispatchEvent(new Event("scroll"));
    });
    expect(document.querySelector(".topology-shell")?.getAttribute("data-more-left")).toBe("true");
  });
});

describe("LinkEdge", () => {
  const Link = edgeTypes.link;
  const draw = (props: Partial<Parameters<typeof Link>[0]>) =>
    render(
      <ReactFlowProvider>
        <svg>
          <Link
            id="e"
            source="a"
            target="b"
            sourceX={0}
            sourceY={0}
            targetX={100}
            targetY={100}
            sourcePosition={Position.Bottom}
            targetPosition={Position.Top}
            {...(props as object)}
          />
        </svg>
      </ReactFlowProvider>,
    );
  const d = () => document.querySelector("path")?.getAttribute("d") ?? "";

  it("follows ELK's route with rounded corners, snapped onto the handles", () => {
    draw({
      data: {
        route: [
          { x: 2, y: 3 },
          { x: 2, y: 50 },
          { x: 98, y: 50 },
          { x: 98, y: 97 },
        ],
      },
    });
    expect(d().startsWith("M 0 0")).toBe(true);
    expect(d()).toContain("Q");
    expect(d().endsWith("L 100 100")).toBe(true);
  });

  it("draws a straight two-point route as is", () => {
    draw({
      sourceX: 0,
      targetX: 0,
      data: {
        route: [
          { x: 1, y: 1 },
          { x: 1, y: 99 },
        ],
      },
    });
    expect(d()).toBe("M 0 0 L 0 100");
  });

  it("falls back to a smooth step when the route no longer meets the handles", () => {
    draw({
      data: {
        route: [
          { x: 500, y: 500 },
          { x: 600, y: 600 },
        ],
        label: "10.0.0.5",
      },
    });
    expect(d()).not.toBe("");
    expect(d()).not.toContain("600");
  });

  it("builds links with their style", () => {
    expect(link("x", "a", "b", GREY, { dashed: true, sideways: true, label: "l" })).toMatchObject({
      sourceHandle: "e",
      targetHandle: "w",
      data: { label: "l" },
      style: { strokeDasharray: "5 5", opacity: 0.8 },
    });
    expect(link("y", "a", "b", "red").style).toMatchObject({ strokeDasharray: undefined, opacity: 1 });
  });
});
