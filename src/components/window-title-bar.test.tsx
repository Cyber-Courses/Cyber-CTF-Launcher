// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocalePreference } from "@/lib/i18n";
import { flush } from "@/test/ui";

// The app's window, as @tauri-apps/api/window gives it; the test drives its maximized state and
// its resize events.
const win = {
  minimize: vi.fn(() => Promise.resolve()),
  toggleMaximize: vi.fn(() => Promise.resolve()),
  close: vi.fn(() => Promise.resolve()),
  destroy: vi.fn(() => Promise.resolve()),
  title: vi.fn(() => Promise.resolve("Set up this machine")),
  isMaximized: vi.fn(() => Promise.resolve(false)),
  onResized: vi.fn((cb: () => void) => {
    resized = cb;
    return Promise.resolve(unlisten);
  }),
};
let resized: () => void = () => {};
const unlisten = vi.fn();
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => win }));
// Inside the app or in a browser.
let inApp = true;
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => inApp }));

const { WindowTitleBar } = await import("@/components/window-title-bar");
const { ShellHeader } = await import("@/components/shell/shell-header");

const AGENTS = {
  linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)",
  windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
  macos: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)",
};
const on = (os: keyof typeof AGENTS, app = true) => {
  inApp = app;
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(AGENTS[os]);
};

const bar = () => document.querySelector<HTMLElement>("[data-tauri-drag-region]");
const buttons = () => screen.queryAllByRole("button").map((b) => b.getAttribute("aria-label"));
/** The window's buttons' box, and the band Tauri resizes the window from that it keeps clear of. */
const controls = () => document.querySelector<HTMLElement>("[data-window-controls]");
const band = () => controls()?.style.getPropertyValue("--band");

/**
 * What Tauri's drag script (tauri 2.12, src/window/scripts/drag.js) does with a double click
 * (the second mousedown) on `target`: maximize or restore the window, on a drag region with
 * nothing clickable between it and the click. An element the pointer passes through
 * (`pointer-events-none`) hands the click to its parent, as the webview does.
 */
function doubleClickOn(target: Element): "internal_toggle_maximize" | null {
  let hit: Element | null = target;
  while (hit?.classList.contains("pointer-events-none")) hit = hit.parentElement;
  const clickable = (el: Element) =>
    ["A", "BUTTON", "INPUT", "SELECT", "TEXTAREA", "LABEL", "SUMMARY"].includes(el.tagName) || el.getAttribute("role") === "button";
  for (let el: Element | null = hit; el; el = el.parentElement) {
    const attr = el.getAttribute("data-tauri-drag-region");
    if (clickable(el) && attr === null) return null;
    if (attr === null) continue;
    if (attr === "false") return null;
    if (attr === "deep") return "internal_toggle_maximize";
    return el === hit ? "internal_toggle_maximize" : null;
  }
  return null;
}

beforeEach(() => {
  vi.clearAllMocks();
  win.isMaximized.mockResolvedValue(false);
  setLocalePreference("en");
});
afterEach(() => {
  vi.restoreAllMocks();
  setLocalePreference("system");
});

describe("the main window's header", () => {
  const header = () => document.querySelector<HTMLElement>("[data-tauri-drag-region].h-12");

  it.each(["linux", "windows"] as const)("is the title bar on %s, with the window's buttons at its end", async (os) => {
    on(os);
    render(<ShellHeader crumbs={["Labs", "SQLi"]} report={null} checkError={null} />);
    await flush();
    const row = header();
    // The breadcrumb's row itself: no other bar above it, and it drags the window.
    expect(document.querySelectorAll(".h-11")).toHaveLength(0);
    expect(row?.textContent).toContain("Labs/SQLi");
    expect(buttons()).toEqual(["Minimize", "Maximize", "Close"]);
    // In the row's top-right corner, after the engine status, over the row's whole height.
    const box = controls();
    expect(box?.parentElement).toBe(row);
    expect(row?.className).toContain("relative");
    expect(box?.className).toMatch(/\bright-0\b/);
    expect(box?.className).toMatch(/\bbottom-0\b/);
    // After the engine status, behind the strip that keeps it clear of them.
    expect(box?.previousElementSibling?.previousElementSibling?.textContent).toMatch(/checking/i);
  });

  it.each([
    ["macos", true],
    ["windows", false],
  ] as const)("is the title bar without buttons of its own on %s (in the app: %s)", async (os, app) => {
    on(os, app);
    render(<ShellHeader crumbs={["Overview"]} report={null} checkError={null} />);
    await flush();
    expect(header()?.hasAttribute("data-tauri-drag-region")).toBe(true);
    expect(buttons()).toEqual([]);
    expect(controls()).toBeNull();
  });
});

describe("WindowTitleBar", () => {
  it.each(["linux", "windows"] as const)("is a secondary window's header row on %s: a drag region, the title and the window's buttons", async (os) => {
    on(os);
    render(<WindowTitleBar title="Settings" />);
    await flush();
    // The row macOS draws, at the same height, the buttons where the traffic lights are not.
    expect(bar()?.className).toContain("h-11");
    expect(bar()?.className).not.toContain("pl-20");
    expect(bar()?.textContent).toContain("Settings");
    expect(buttons()).toEqual(["Minimize", "Maximize", "Close"]);
    expect(controls()?.parentElement).toBe(bar());
  });

  it("clears the traffic lights on macOS, with no buttons of its own", async () => {
    on("macos");
    const { rerender } = render(<WindowTitleBar />);
    await flush();
    expect(bar()?.className).toContain("pl-20");
    expect(bar()?.className).toContain("h-11");
    expect(bar()?.textContent).toBe("");
    expect(buttons()).toEqual([]);
    rerender(<WindowTitleBar title="Settings" />);
    expect(bar()?.textContent).toBe("Settings");
  });

  it("is nothing in a browser, but a toolbar keeps its controls there", () => {
    on("windows", false);
    const { rerender } = render(<WindowTitleBar title="Settings" />);
    expect(bar()).toBeNull();
    rerender(
      <WindowTitleBar title="Settings" toolbar>
        <button type="button">Skip</button>
      </WindowTitleBar>,
    );
    expect(buttons()).toEqual([null]);
    expect(screen.getByRole("button").textContent).toBe("Skip");
  });

  it("shows no title when the page gives none, as macOS", async () => {
    on("linux");
    render(<WindowTitleBar />);
    await flush();
    expect(bar()?.textContent).toBe("");
    expect(win.title).not.toHaveBeenCalled();
  });

  // Regression: the shell window's header said "attack box" twice on Windows, the window's own
  // title ("<lab> · attack box", which the bar showed) beside the page's state.
  it.each(["linux", "windows", "macos"] as const)("says what the page puts in its toolbar once on %s, not the window's title too", async (os) => {
    on(os);
    win.title.mockResolvedValueOnce("SQLi lab · attack box");
    render(
      <WindowTitleBar toolbar>
        <span>attack box</span>
      </WindowTitleBar>,
    );
    await flush();
    expect(bar()?.textContent.match(/attack box/g)).toEqual(["attack box"]);
    expect(win.title).not.toHaveBeenCalled();
  });

  it("puts the page's own controls before the window's buttons", async () => {
    on("windows");
    render(
      <WindowTitleBar toolbar>
        <button type="button">Skip</button>
      </WindowTitleBar>,
    );
    await flush();
    expect(buttons()).toEqual([null, "Minimize", "Maximize", "Close"]);
  });

  it("minimizes, maximizes and closes the window through its buttons", async () => {
    on("linux");
    const user = userEvent.setup();
    render(<WindowTitleBar title="Cyber CTF" />);
    await flush();
    await user.click(screen.getByRole("button", { name: "Minimize" }));
    expect(win.minimize).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Maximize" }));
    expect(win.toggleMaximize).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Close" }));
    // A close request, as the system's button makes: the quit guard can hold it while a lab
    // deploys (destroy would skip it).
    expect(win.close).toHaveBeenCalledTimes(1);
    expect(win.destroy).not.toHaveBeenCalled();
  });

  it("reaches its buttons from the keyboard", async () => {
    on("windows");
    const user = userEvent.setup();
    render(<WindowTitleBar title="Cyber CTF" />);
    await flush();
    await user.tab();
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Minimize");
    await user.tab();
    await user.keyboard("{Enter}");
    expect(win.toggleMaximize).toHaveBeenCalledTimes(1);
    await user.tab();
    await user.keyboard(" ");
    expect(win.close).toHaveBeenCalledTimes(1);
  });

  it("says Restore, with the restore icon, while the window is maximized, following its resizes", async () => {
    on("linux");
    win.isMaximized.mockResolvedValue(true);
    const { unmount } = render(<WindowTitleBar title="Cyber CTF" />);
    await flush();
    expect(buttons()).toEqual(["Minimize", "Restore", "Close"]);
    expect(document.querySelector("[data-icon=restore]")).not.toBeNull();
    // Restored (from the bar, a double-click or the window manager): the window was resized.
    win.isMaximized.mockResolvedValue(false);
    await act(async () => {
      resized();
      await flush();
    });
    expect(buttons()).toEqual(["Minimize", "Maximize", "Close"]);
    expect(document.querySelector("[data-icon=maximize]")).not.toBeNull();
    unmount();
    await flush();
    expect(unlisten).toHaveBeenCalled();
  });

  it("keeps its buttons clear of the band the window resizes from, and in the very corner once maximized", async () => {
    // Linux: Tauri's band runs along every edge, so the buttons start below and left of it.
    on("linux");
    const { unmount } = render(<WindowTitleBar />);
    await flush();
    expect(band()).toBe("5px");
    expect(controls()?.className).toContain("top-(--band)");
    expect(controls()?.className).toContain("right-(--band)");
    // Maximized: no band, flush to the screen's corner.
    win.isMaximized.mockResolvedValue(true);
    await act(async () => {
      resized();
      await flush();
    });
    expect(band()).toBe("0px");
    unmount();
    // Windows: the band is along the top only (the shadow takes the sides).
    on("windows");
    win.isMaximized.mockResolvedValue(false);
    render(<WindowTitleBar />);
    await flush();
    expect(band()).toBe("5px");
    expect(controls()?.className).not.toContain("right-(--band)");
  });

  it("maximizes or restores the window on a double-click on the bar or its title, not on a button", async () => {
    on("linux");
    render(
      <WindowTitleBar title="Cyber CTF" toolbar>
        <span data-tauri-drag-region>Connected</span>
      </WindowTitleBar>,
    );
    await flush();
    const target = bar();
    if (!target) throw new Error("no bar");
    expect(doubleClickOn(target)).toBe("internal_toggle_maximize");
    expect(doubleClickOn(screen.getByText("Cyber CTF"))).toBe("internal_toggle_maximize");
    expect(doubleClickOn(screen.getByText("Connected"))).toBe("internal_toggle_maximize");
    // The strip that keeps the row's content clear of the buttons drags too.
    const spacer = controls()?.previousElementSibling;
    if (!spacer) throw new Error("no spacer");
    expect(doubleClickOn(spacer)).toBe("internal_toggle_maximize");
    for (const name of ["Minimize", "Maximize", "Close"]) expect(doubleClickOn(screen.getByRole("button", { name }))).toBeNull();
  });

  it("labels its buttons in the player's language", async () => {
    on("linux");
    setLocalePreference("fr");
    render(<WindowTitleBar title="Réglages" />);
    await flush();
    expect(buttons()).toEqual(["Réduire", "Agrandir", "Fermer"]);
  });

  it("draws the window's edge on Linux only, where no system frame does, and not while maximized", async () => {
    const edge = () => document.querySelector("[aria-hidden].fixed.inset-0");
    on("windows");
    const { unmount } = render(<WindowTitleBar title="Cyber CTF" />);
    await flush();
    expect(edge()).toBeNull();
    unmount();
    on("linux");
    render(<WindowTitleBar title="Cyber CTF" />);
    await flush();
    expect(edge()).not.toBeNull();
    win.isMaximized.mockResolvedValue(true);
    await act(async () => {
      resized();
      await flush();
    });
    expect(edge()).toBeNull();
  });
});
