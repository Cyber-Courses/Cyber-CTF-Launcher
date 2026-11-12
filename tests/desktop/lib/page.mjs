// What the specs read of, and do to, the app's page in the page itself, and the selectors they
// find its parts by. The embedded WebDriver server (tauri-plugin-wdio-webdriver) answers Get Element
// Text with the element's `textContent`, not the rendered text a browser's driver gives: the rows
// of a block run together with nothing between them (xterm's rows read "└─$ whoamiplayer┌──(…"),
// so a spec that needs the lines reads them here, through Execute Script; and every press it sends
// carries `detail` 1, so it can't double-click. No dependency of its own, so the unit tests load it
// in jsdom.

/**
 * The main window's title bar: the main column's header (ShellHeader), which drags the window and
 * whose double-click maximizes it. The window has other drag regions, the sidebar's top row and the
 * window buttons' spacer inside the header, so a bare `[data-tauri-drag-region]` matches three on
 * Linux and Windows, which WebdriverIO's strict `$` refuses.
 */
export const TITLE_BAR = "main > [data-tauri-drag-region]";

/**
 * The text of the terminal on this page (xterm.js's DOM renderer: a `.xterm-rows` block of one
 * `<div>` per row), one line per row, each without its trailing blanks; "" when the page has no
 * terminal. Run in the page (`b.execute(terminalText)`), so it names nothing outside itself.
 * @param {Document} [doc]
 * @returns {string}
 */
export function terminalText(doc = document) {
  const rows = doc.querySelector(".xterm-rows");
  if (!rows) return "";
  return Array.from(rows.children)
    .map((row) => (row.textContent || "").replace(/\u00a0/g, " ").replace(/\s+$/, ""))
    .join("\n");
}

/**
 * The rows of the Labs list on this page, in its order (the labs imported from a folder first):
 * each one's open button's name (`label`: "Open <title>", or "Open <title>, imported from a
 * folder"), its title (the button's text) and its slug (the first word of the line under it). Run
 * in the page (`b.execute(labRows)`).
 * @returns {{ label: string, title: string, slug: string }[]}
 */
export function labRows() {
  return Array.from(document.querySelectorAll("main button[data-row-open]")).map((open) => ({
    label: open.getAttribute("aria-label") || "",
    title: (open.textContent || "").trim(),
    slug: ((open.nextElementSibling && open.nextElementSibling.textContent) || "").trim().split(/\s/)[0],
  }));
}

/**
 * The second press of a double-click on the element at `selector`, sent as the system sends it: a
 * `mousedown` with `detail` 2, on which Tauri's drag-region handler (its drag.js) toggles the
 * window's maximize; the first press, `detail` 1, would start a window drag. The press lands on a
 * point of the element where what the pointer hits is the element itself or a drag region inside
 * it, nothing clickable over it, as Tauri requires. The point, or null when the element has none.
 * Run in the page (`b.execute(doubleClickPress, selector)`), so it names nothing outside itself.
 * @param {string} selector
 * @returns {{ x: number, y: number } | null}
 */
export function doubleClickPress(selector) {
  const el = document.querySelector(selector);
  if (!el) return null;
  const box = el.getBoundingClientRect();
  const y = Math.round(box.top + box.height / 2);
  for (let step = 10; step < 30; step++) {
    // From the middle rightwards, then from the left: the breadcrumb starts the bar, the window
    // buttons end it.
    const x = Math.round(box.left + (box.width * (step % 20)) / 20);
    const hit = document.elementFromPoint(x, y);
    if (!hit || !el.contains(hit) || !hit.hasAttribute("data-tauri-drag-region")) continue;
    const init = { bubbles: true, cancelable: true, composed: true, view: window, button: 0, buttons: 1, detail: 2, clientX: x, clientY: y };
    hit.dispatchEvent(new MouseEvent("mousedown", init));
    hit.dispatchEvent(new MouseEvent("mouseup", { ...init, buttons: 0 }));
    return { x, y };
  }
  return null;
}
