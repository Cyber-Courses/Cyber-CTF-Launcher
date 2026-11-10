// @vitest-environment jsdom
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/app-shell";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

const AGENTS = {
  windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
  linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)",
  macos: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)",
};
type Tauri = { isTauri?: boolean };
afterEach(() => {
  delete (globalThis as Tauri).isTauri;
  vi.restoreAllMocks();
});

describe("AppShell", () => {
  it("renders the shell and walks every screen", async () => {
    localStorage.setItem("cyberctf.onboarded", "1");
    installTauri();
    const user = userEvent.setup();
    render(<AppShell />);
    const nav = await screen.findByRole("navigation");
    for (const name of ["Labs", "Events", "Machine", "Servers", "Cloud", "Overview"]) {
      await user.click(within(nav).getAllByRole("button", { name: new RegExp(name, "i") })[0]);
      await act(async () => {
        await new Promise((r) => setTimeout(r, 50));
      });
    }
    expect(document.body.textContent).toBeTruthy();
  });

  it("shows a cloud launch asked from the website while the onboarding is up", async () => {
    // First run: no onboarded flag.
    installTauri();
    render(<AppShell />);
    await screen.findAllByText(/welcome/i);
    await act(() =>
      emit("launch-confirm", {
        sessionId: "sess-1",
        labName: "S3 leak",
        repository: "cyber-courses/lab-s3",
        commit: "0123456789abcdef",
        place: "cloud",
        target: "AWS sandbox",
        privileged: false,
        expiresAtMs: Date.now() + 180_000,
      }),
    );
    await flush();
    expect(screen.getByRole("alertdialog").textContent).toContain("AWS sandbox");
    // The onboarding ends before the player answers: the question is still there.
    fireEvent.click(screen.getByRole("button", { name: /skip setup/i }));
    await flush();
    expect(screen.queryByRole("navigation")).toBeTruthy();
    expect(screen.getByRole("alertdialog").textContent).toContain("AWS sandbox");
  });

  it.each(["windows", "linux", "macos"] as const)("runs the sidebar and the header to the window's top on %s, with no title bar of its own", async (os) => {
    localStorage.setItem("cyberctf.onboarded", "1");
    installTauri();
    (globalThis as Tauri).isTauri = true;
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(AGENTS[os]);
    const { container } = render(<AppShell />);
    await screen.findByRole("navigation");
    await flush(50);
    // The window is the sidebar beside the main column: nothing above them.
    const root = container.querySelector("aside")?.parentElement;
    expect(root?.parentElement).toBe(container);
    expect(root?.className).not.toContain("flex-col");
    const main = container.querySelector("main");
    expect(main?.parentElement).toBe(root);
    // The main column's first row is its header, the window's title bar, at the macOS height.
    const header = main?.firstElementChild;
    expect(header?.hasAttribute("data-tauri-drag-region")).toBe(true);
    expect(header?.className).toContain("h-12");
    // Drag regions only in the sidebar's top and that header: no other bar.
    for (const region of container.querySelectorAll("[data-tauri-drag-region]")) {
      expect(region.closest("aside") ?? region.closest("main > [data-tauri-drag-region]")).not.toBeNull();
    }
    // The window's buttons, once, at the header's end, where the system draws none.
    const names = screen.queryAllByRole("button", { name: /^(Minimize|Maximize|Close)$/ }).map((b) => b.getAttribute("aria-label"));
    expect(names).toEqual(os === "macos" ? [] : ["Minimize", "Maximize", "Close"]);
    if (os !== "macos") expect(header?.querySelector("[data-window-controls]")).not.toBeNull();
    // The brand on the header's line where no traffic lights take the sidebar's top.
    const brand = container.querySelector("aside > [data-tauri-drag-region]");
    expect(brand?.textContent).toContain(os === "macos" ? "" : "Cyber CTF");
    if (os !== "macos") expect(brand?.className).toContain("h-12");
  });
});
