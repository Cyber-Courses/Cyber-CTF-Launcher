// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TermEvent } from "@/lib/tauri";
import { installTauri, stream } from "@/test/tauri";
import { flush } from "@/test/ui";

// xterm draws on a canvas jsdom lacks: a stand-in that, like the real one, can't be written to
// once disposed.
const terms = vi.hoisted(() => [] as { writes: string[]; disposed: boolean }[]);
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    state = { writes: [] as string[], disposed: false };
    constructor() {
      terms.push(this.state);
    }
    loadAddon() {}
    open() {}
    focus() {}
    onData = () => ({ dispose() {} });
    onResize = () => ({ dispose() {} });
    write(data: string) {
      if (this.state.disposed) throw new Error("write on a disposed terminal");
      this.state.writes.push(data);
    }
    dispose() {
      this.state.disposed = true;
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

const { useTerminalSession } = await import("@/app/shell/use-terminal-session");

describe("useTerminalSession", () => {
  it("drops the shell's output once its terminal is disposed (Reconnect, or the window closing)", async () => {
    let send: (e: TermEvent) => void = () => {};
    installTauri({
      terminal_open: (args) => {
        send = (e) => {
          stream(args, e);
        };
        return 7;
      },
      terminal_close: () => null,
    });
    const host = { current: document.createElement("div") };
    const setState = vi.fn();
    const { unmount } = renderHook(() => {
      useTerminalSession(host, { id: "lab", kind: "lab", runtime: "DOCKER", attempt: 0 }, setState);
    });
    await flush();
    send({ kind: "data", data: "$ " });
    expect(terms[0].writes).toEqual(["$ "]);
    unmount();
    expect(() => {
      send({ kind: "data", data: "late output" });
    }).not.toThrow();
    send({ kind: "exit", code: 0 });
    expect(terms[0].writes).toEqual(["$ "]);
    expect(setState).not.toHaveBeenCalledWith({ kind: "ended", code: 0 });
  });
});
