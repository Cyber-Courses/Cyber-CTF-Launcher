// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { terminalText } from "./lib/page.mjs";

// What the specs read of the app's page in the page itself (lib/page.mjs), against the markup the
// app draws.

/** xterm.js's DOM renderer: one `<div>` of `<span>`s per row under `.xterm-rows`. */
function xtermRows(rows: string[][]) {
  const block = document.createElement("div");
  block.className = "xterm-rows";
  for (const spans of rows) {
    const row = document.createElement("div");
    for (const text of spans) {
      const span = document.createElement("span");
      span.textContent = text;
      row.append(span);
    }
    block.append(row);
  }
  document.body.append(block);
  return block;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("the terminal's text", () => {
  // jarvis, debian13-quick, e2e.lab attack-shell: the Kali box's prompt, `whoami` and its answer.
  const screen = [
    ["┌──(", "player㉿attackbox", ")-[", "~", "]"],
    ["└─", "$ ", "whoami"],
    ["player"],
    ["┌──(", "player㉿attackbox", ")-[", "~", "]"],
    ["└─", "$ ", "    "],
  ];

  it("is a line per row, so the answer stands on its own line", () => {
    xtermRows(screen);
    const lines = terminalText().split("\n");
    expect(lines).toEqual(["┌──(player㉿attackbox)-[~]", "└─$ whoami", "player", "┌──(player㉿attackbox)-[~]", "└─$"]);
    expect(lines.some((l) => l.trim() === "player")).toBe(true);
  });

  it("is not the block's textContent, the embedded WebDriver's getText, which glues the answer to its command", () => {
    const block = xtermRows(screen);
    // What the spec read before: "no answer to `whoami`" with the answer on screen.
    expect(block.textContent).toContain("└─$ whoamiplayer┌──(");
    expect(block.textContent.split("\n").some((l) => l.trim() === "player")).toBe(false);
  });

  it("reads a no-break space as a space, and is empty with no terminal on the page", () => {
    xtermRows([["$\u00a0cat\u00a0/shared/x"], ["seen-from-the-box\u00a0\u00a0"]]);
    expect(terminalText()).toBe("$ cat /shared/x\nseen-from-the-box");
    document.body.replaceChildren();
    expect(terminalText()).toBe("");
  });
});
