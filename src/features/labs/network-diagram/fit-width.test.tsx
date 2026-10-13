// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NetworkDiagram } from "@/features/labs/network-diagram";
import { FIT_WIDTH_KEY, fitShellSize, readFitWidth, writeFitWidth } from "@/features/labs/network-diagram/fit-width";
import type { Machine } from "@/features/labs/network-diagram/model";
import { flush } from "@/test/ui";

// As in diagram.test.tsx: jsdom measures nothing, so pretend React Flow did, and report a fixed
// width for every observed element (the shell).
vi.mock("@xyflow/react", async (orig) => {
  const mod = await orig<typeof import("@xyflow/react")>();
  return { ...mod, useNodesInitialized: () => true };
});

let shellWidth = 300;
class SizedObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(el: Element) {
    this.cb([{ target: el, contentRect: { width: shellWidth, height: 400 } } as unknown as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  unobserve() {}
  disconnect() {}
}

const machine = (name: string, i: number): Machine => ({
  name,
  state: "running",
  image: `${name}:latest`,
  ip: `10.42.0.${i + 2}`,
  ports: [{ published: 8000 + i, target: 80 }],
  interfaces: [{ network: "default", ip: `10.42.0.${i + 2}` }],
  services: [],
  infra: false,
});
const wideLab = ["a", "b", "c", "d"].map(machine);

const settle = async () => {
  for (let i = 0; i < 10; i++) await flush(20);
};
const shell = () => document.querySelector<HTMLElement>(".topology-shell")!;
const canvasWidth = () => parseFloat(document.querySelector<HTMLElement>(".topology-canvas")!.style.width);

beforeEach(() => {
  localStorage.clear();
  shellWidth = 300;
  vi.stubGlobal("ResizeObserver", SizedObserver);
  vi.stubGlobal(
    "DOMMatrixReadOnly",
    class {
      m22 = 1;
    },
  );
});
const scrollBy = HTMLElement.prototype.scrollBy;
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  HTMLElement.prototype.scrollBy = scrollBy;
});

describe("fitShellSize", () => {
  it("keeps the default height until the graph is measured", () => {
    expect(fitShellSize(null, 800)).toEqual({ canvas: 800, height: 430 });
  });
  it("scales a wide graph down to the shell's width, its height following", () => {
    // (600 - 48) / 2000 = 0.276: 1000 * 0.276 + 24 = 300.
    expect(fitShellSize({ w: 2000, h: 1000 }, 600)).toEqual({ canvas: 600, height: 300 });
  });
  it("stops at the minimum zoom and keeps a floor and a ceiling on the height", () => {
    expect(fitShellSize({ w: 10_000, h: 100 }, 600)).toEqual({ canvas: 600, height: 140 });
    expect(fitShellSize({ w: 10_000, h: 10_000 }, 600)).toEqual({ canvas: 600, height: 720 });
  });
});

describe("the remembered choice", () => {
  it("is off by default, and stored on or cleared", () => {
    expect(readFitWidth()).toBe(false);
    writeFitWidth(true);
    expect(localStorage.getItem(FIT_WIDTH_KEY)).toBe("1");
    expect(readFitWidth()).toBe(true);
    writeFitWidth(false);
    expect(localStorage.getItem(FIT_WIDTH_KEY)).toBeNull();
  });

  it("is off, and not saved, when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(readFitWidth()).toBe(false);
    expect(() => writeFitWidth(true)).not.toThrow();
  });
});

describe("Fit to width", () => {
  it("is offered only for a lab wider than its card", async () => {
    shellWidth = 4000;
    render(<NetworkDiagram machines={wideLab} />);
    await settle();
    expect(screen.queryByRole("button", { name: /Fit to width/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Scroll for more/ })).toBeNull();
  });

  it("shrinks a wide lab into the card, says there is more while it scrolls, and is remembered", async () => {
    const scrolled = vi.fn();
    HTMLElement.prototype.scrollBy = scrolled;
    const user = userEvent.setup();
    const { unmount } = render(<NetworkDiagram machines={wideLab} />);
    await settle();
    // Scrolling: wider than the card, with the cue (which scrolls on) and the toggle off.
    expect(canvasWidth()).toBeGreaterThan(300);
    expect(shell().getAttribute("data-more-right")).toBe("true");
    await user.click(screen.getByRole("button", { name: /Scroll for more/ }));
    expect(scrolled).toHaveBeenCalledWith({ left: 240, behavior: "smooth" });
    const toggle = screen.getByRole("button", { name: /Fit to width/ });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");

    // Fitted: the canvas is the card, nothing to scroll, no cue; the toggle stays to undo it.
    await user.click(toggle);
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(canvasWidth()).toBe(300);
    expect(shell().getAttribute("data-fit")).toBe("true");
    expect(shell().getAttribute("data-more-right")).toBeNull();
    expect(screen.queryByRole("button", { name: /Scroll for more/ })).toBeNull();
    expect(localStorage.getItem(FIT_WIDTH_KEY)).toBe("1");

    // Remembered: the next diagram opens fitted.
    unmount();
    render(<NetworkDiagram machines={wideLab} />);
    await settle();
    expect(screen.getByRole("button", { name: /Fit to width/ }).getAttribute("aria-pressed")).toBe("true");
    expect(canvasWidth()).toBe(300);

    // And undone: back to the readable zoom, scrolling sideways.
    await user.click(screen.getByRole("button", { name: /Fit to width/ }));
    expect(canvasWidth()).toBeGreaterThan(300);
    expect(shell().getAttribute("data-fit")).toBeNull();
    expect(localStorage.getItem(FIT_WIDTH_KEY)).toBeNull();
  });
});
