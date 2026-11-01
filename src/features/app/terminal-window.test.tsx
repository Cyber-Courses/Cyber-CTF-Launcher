// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { commands, installTauri, rejects, rejectWith } from "@/test/tauri";
import { flush } from "@/test/ui";

// The attack box shell window when its pseudo-terminal can't open, and when the system terminal
// can't: each is said in the window, and nothing crashes.

// The window's query string: which lab's shell it shows.
vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams("id=lab-sqli&runtime=DOCKER") }));

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

  it("swallows a system terminal that can't open, without crashing the window", async () => {
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
    // Still the terminal, still connected: the failure went nowhere.
    expect(screen.getByText("attack box")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
