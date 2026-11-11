// The integrated title bar on Linux and Windows, where the app's windows have no system frame and
// each page draws its own (src/components/window-title-bar.tsx): the main window's Minimize,
// Maximize/Restore and Close, a double-click on the header, and a secondary window's (Settings)
// own bar and Close. Each button is checked by what the window then is (`is_maximized`,
// `is_minimized`, the window gone, the app quit), not by the button's look. macOS keeps the
// system's frame and traffic lights: skipped there.
import { closeWindow, finish, goTo, openApp, invokeCmd, ownFrame, results, sleep, switchToWindow } from "../lib/support.mjs";

export const needs = ["own-frame"];
const r = results();
if (!ownFrame) r.skipAll("macOS keeps the system's window frame: the app draws no window controls there");

const state = (b, what, label = "main") => invokeCmd(b, `plugin:window|is_${what}`, { label }, { timeoutMs: 15000 });
const control = (b, name) => b.$(`[data-window-controls] button[aria-label=${JSON.stringify(name)}]`);
/** Waits until window `label` is (or isn't) `what`; true when it got there. */
const becomes = (b, what, want, label = "main") =>
  b
    .waitUntil(async () => (await state(b, what, label).catch(() => null)) === want, { timeout: 15000, interval: 500 })
    .then(() => true)
    .catch(() => false);

let proc;
let b;
try {
  ({ proc, b } = await openApp());

  await r.step("controls", "the main window's header ends with Minimize, Maximize and Close", async () => {
    for (const name of ["Minimize", "Maximize", "Close"]) {
      if (!(await (await control(b, name)).isExisting())) throw new Error(`no ${name} button in [data-window-controls]`);
    }
    if ((await b.$$("[data-tauri-drag-region]")).length === 0) throw new Error("no drag region in the header");
  });

  await (await control(b, "Maximize")).click();
  r.check("maximize", await becomes(b, "maximized", true), "Maximize maximizes the window");
  r.check("restore-label", await (await control(b, "Restore")).isExisting(), "the button then reads Restore");
  await (await control(b, "Restore")).click();
  r.check("restore", await becomes(b, "maximized", false), "Restore brings the window back to its size");

  // Tauri's own handler on any `data-tauri-drag-region`: a double-click toggles the maximize.
  const header = await b.$("main [data-tauri-drag-region], header [data-tauri-drag-region], [data-tauri-drag-region]");
  await header.doubleClick();
  const doubled = await becomes(b, "maximized", true);
  r.check("double-click", doubled, "a double-click on the header maximizes the window");
  if (doubled) {
    await (await control(b, "Restore")).click();
    await becomes(b, "maximized", false);
  }

  // A secondary window draws the same bar, with its title.
  await goTo(b, "Settings");
  await switchToWindow(b, "settings");
  await r.step("settings-bar", "the Settings window has its own title bar and controls", async () => {
    await b.$("[data-window-controls]").waitForExist({ timeout: 15000 });
    const text = await b.$("body").getText();
    if (!/Settings/.test(text)) throw new Error("its bar doesn't say Settings");
  });
  await (await control(b, "Close")).click();
  await b.switchToWindow("main");
  const closed = await b
    .waitUntil(async () => !(await b.getWindowHandles()).includes("settings"), { timeout: 15000, interval: 500 })
    .then(() => true)
    .catch(() => false);
  r.check("settings-close", closed, "Close on the Settings window closes it, the main window stays");
  if (!closed) await closeWindow(b, "settings");

  await (await control(b, "Minimize")).click();
  r.check("minimize", await becomes(b, "minimized", true), "Minimize minimizes the window");
  // Back up, so the last check can press Close: WebDriver's Set Window Rect restores a window.
  await b.setWindowRect(null, null, 1280, 820).catch(() => {});
  await sleep(1500);

  // Close on the main window quits the app when nothing is deploying (the quit guard holds it
  // otherwise: src-tauri/src/app/quit.rs).
  await (await control(b, "Close")).click();
  const quit = await new Promise((resolve) => {
    if (proc.exitCode !== null) resolve(true);
    proc.once("exit", () => resolve(true));
    setTimeout(() => resolve(proc.exitCode !== null), 20000);
  });
  r.check("main-close", quit, "Close on the main window quits the app (nothing deploying)");
} catch (e) {
  r.fail("error", e.message);
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
