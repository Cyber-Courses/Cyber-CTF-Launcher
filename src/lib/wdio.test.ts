// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { exposeTerminalText, WDIO_TERMINAL_TEXT } from "./wdio";

/** A stand-in for xterm's buffer: `lines` from its top, the viewport `rows` high from `viewportY`. */
function term(lines: string[], rows: number, viewportY: number) {
  return {
    rows,
    buffer: {
      active: {
        viewportY,
        getLine: (y: number) => (y < lines.length ? { translateToString: (trim?: boolean) => (trim ? lines[y].trimEnd() : lines[y]) } : undefined),
      },
    },
  };
}

const read = (element: HTMLElement) => (element as HTMLElement & Record<string, () => string>)[WDIO_TERMINAL_TEXT]();

describe("exposeTerminalText", () => {
  it("reads the viewport's rows, each trimmed, a row past the buffer's end blank", () => {
    const element = document.createElement("div");
    exposeTerminalText(element, term(["scrolled away", "$ whoami   ", "player"], 3, 1));
    expect(read(element)).toBe("$ whoami\nplayer\n");
  });

  it("follows the terminal: what it reads is the buffer as it is now", () => {
    const element = document.createElement("div");
    const lines = ["$ "];
    exposeTerminalText(element, term(lines, 1, 0));
    lines[0] = "$ ls";
    expect(read(element)).toBe("$ ls");
  });

  it("is taken away with the terminal", () => {
    const element = document.createElement("div");
    const unexpose = exposeTerminalText(element, term([], 1, 0));
    expect(WDIO_TERMINAL_TEXT in element).toBe(true);
    unexpose();
    expect(WDIO_TERMINAL_TEXT in element).toBe(false);
  });
});
