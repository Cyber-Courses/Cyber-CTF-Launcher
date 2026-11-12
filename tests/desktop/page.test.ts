// @vitest-environment jsdom
import { screen as rendered, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/app-shell";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { setAttackDesktop } from "@/lib/settings";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";
import { choiceState, terminalText, TITLE_BAR } from "./lib/page.mjs";

// What the specs read of the app's page in the page itself, and the selectors they find its parts
// by (lib/page.mjs), against the markup the app draws.

type Tauri = { isTauri?: boolean };

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
  delete (globalThis as Tauri).isTauri;
  vi.restoreAllMocks();
});

describe("the main window's title bar", () => {
  const AGENTS = {
    windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
    linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)",
  };

  it.each(["linux", "windows"] as const)("is the main column's header alone on %s, where the window has three drag regions", async (os) => {
    localStorage.setItem("cyberctf.onboarded", "1");
    installTauri();
    (globalThis as Tauri).isTauri = true;
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(AGENTS[os]);
    render(createElement(AppShell));
    await rendered.findByRole("navigation");
    await flush(50);
    const bars = document.querySelectorAll(TITLE_BAR);
    expect(bars).toHaveLength(1);
    expect(bars[0]).toBe(document.querySelector("main")?.firstElementChild);
    expect(bars[0].querySelector("[data-window-controls]")).not.toBeNull();
    // The rig's title-bar spec (debian13-quick e2e.shell, Windows on Arm): the selector it gave
    // WebdriverIO's strict `$`, which "resolved to 3 elements": the sidebar's top row, the header and
    // the window buttons' spacer in it.
    const before = "main [data-tauri-drag-region], header [data-tauri-drag-region], [data-tauri-drag-region]";
    expect(document.querySelectorAll(before)).toHaveLength(3);
  });
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

describe("a choice's state, said by a failed step", () => {
  // settings/attack-desktop (#319): "Desktop wasn't saved" alone, once under load, without whether
  // the Settings window had the focus, which option was checked or what was stored.
  const GROUP = "Attack VM desktop";
  const KEY = "cyberctf.attackbox.desktop";

  it("says the window's focus, each option of the app's radio group checked or not, the stored setting and the focused element", async () => {
    setAttackDesktop(false);
    installTauri();
    const user = userEvent.setup();
    render(createElement(AttackBoxRows, { onSaved: () => {} }));
    await flush();
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const before = choiceState(GROUP, KEY);
    expect(before).toMatch(new RegExp(`^window focus: false, visibility: ${document.visibilityState}; `));
    expect(before).toContain(`; ${GROUP}: Headless Default=true, Desktop=false; stored ${KEY}: "false"; focused: none; page: `);
    // What the window showed, from its start: here the rows above the choice.
    expect(before).toContain("; page: Attack box image");
    await user.click(rendered.getByText("Desktop"));
    expect(choiceState(GROUP, KEY)).toContain(`; ${GROUP}: Headless Default=false, Desktop=true; stored ${KEY}: "true"; focused: radio "Desktop"; `);
  });

  it("says when the group isn't on the page and nothing is stored", () => {
    localStorage.removeItem(KEY);
    expect(choiceState(GROUP, KEY)).toContain(`; ${GROUP}: not on the page; stored ${KEY}: null; focused: none; page: `);
  });
});
