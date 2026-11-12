// What the specs read of the app's page in the page itself. The embedded WebDriver server
// (tauri-plugin-wdio-webdriver) answers Get Element Text with the element's `textContent`, not
// the rendered text a browser's driver gives: the rows of a block run together with nothing between
// them (xterm's rows read "└─$ whoamiplayer┌──(player…"), so a spec that needs the lines reads
// them here, through Execute Script. No dependency of its own, so the unit tests load it in jsdom.

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
