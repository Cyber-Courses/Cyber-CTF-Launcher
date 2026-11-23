// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { commands, installTauri, rejects, rejectWith } from "@/test/tauri";
import { flush } from "@/test/ui";

// The attack box shell window when its pseudo-terminal can't open, and when the system terminal
// can't: each is said in the window, and nothing crashes.

// The window's query string: which lab's shell it shows.
const query = vi.hoisted(() => ({ value: "id=lab-sqli&runtime=DOCKER" }));
vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams(query.value) }));

// xterm draws on a canvas jsdom lacks: a stand-in with the surface the hook uses.
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    loadAddon() {}
    open() {}
    focus() {}
    onData = () => ({ dispose() {} });
    onResize = () => ({ dispose() {} });
    write() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

const { default: ShellWindow } = await import("@/app/shell/page");

describe("the terminal window", () => {
  it("says it couldn't connect when the shell can't open, with the reason and Reconnect", async () => {
    let attempts = 0;
    installTauri({
      terminal_open: () => {
        if (++attempts === 1) rejectWith({ code: "other", message: "no attack box running for lab-sqli" });
        return 7;
      },
      terminal_close: () => null,
    });
    const user = userEvent.setup();
    render(<ShellWindow />);
    await flush();
    expect(screen.getByText("couldn't connect")).toBeTruthy();
    expect(screen.getByText("no attack box running for lab-sqli")).toBeTruthy();
    // Reconnect opens a fresh session; this one connects.
    await user.click(screen.getByRole("button", { name: /Reconnect/ }));
    await flush();
    expect(screen.getByText("attack box")).toBeTruthy();
    expect(screen.queryByText("couldn't connect")).toBeNull();
    expect(attempts).toBe(2);
  });

  it("logs a Close the window refuses, as the other windows do", async () => {
    installTauri({
      terminal_open: () => rejectWith({ code: "other", message: "no attack box running for lab-sqli" }),
      terminal_close: () => null,
      "plugin:window|close": () => rejectWith("the window is gone"),
    });
    const logged = vi.spyOn(console, "warn").mockImplementation(() => {});
    const user = userEvent.setup();
    render(<ShellWindow />);
    await flush();
    await user.click(screen.getByRole("button", { name: /^Close$/ }));
    await flush();
    expect(logged).toHaveBeenCalledWith("closing the window:", "the window is gone");
    logged.mockRestore();
  });

  it("says when the system terminal can't open, without crashing the window", async () => {
    const calls = installTauri({
      terminal_open: () => 7,
      terminal_close: () => null,
      lab_attack_shell: rejects({ code: "other", message: "osascript failed" }),
    });
    const user = userEvent.setup();
    render(<ShellWindow />);
    await flush();
    expect(screen.getByText("attack box")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /System terminal/ }));
    await flush();
    expect(commands(calls)).toContain("lab_attack_shell");
    // Still the terminal, still connected; the failure is said, with why.
    expect(screen.getByText("attack box")).toBeTruthy();
    expect(await screen.findByText("Couldn't open the system terminal")).toBeTruthy();
    expect(screen.getByText(/osascript failed/)).toBeTruthy();
  });

  it("opens no session for a runtime it doesn't know, and says the window has no lab", async () => {
    // Regression: an unknown runtime in the address was cast to one and the shell asked for the
    // lab's attack box as if it were a container lab.
    const calls = installTauri();
    query.value = "id=lab-sqli&runtime=PODS";
    try {
      render(<ShellWindow />);
      await flush();
      expect(screen.getByRole("alert").textContent).toMatch(/opened for a lab/);
      expect(commands(calls)).not.toContain("terminal_open");
    } finally {
      query.value = "id=lab-sqli&runtime=DOCKER";
    }
  });
});
