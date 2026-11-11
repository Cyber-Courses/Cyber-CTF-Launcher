// Settings, in its own window (label `settings`, opened from the sidebar): the theme and the
// language, each followed by the main window too (both are saved on this computer and every
// window follows them through the `storage` event), and the folder shared with the attack box,
// picked through the system's folder picker (answered by `answerPickers`). Every setting is put
// back as it was, and the attack VM's desktop (headless or Kali's desktop) is chosen and put
// back to headless. The first run's wizard is the `onboarding` spec; whether the box sees the shared
// folder is the attack-shell spec's and the integration tests' (`box-*`).
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { answerPickers, closeWindow, finish, goTo, invokeCmd, openApp, results, sleep, switchToWindow } from "../support.mjs";

const r = results();

/** The appearance class on the page's root: `light`, `black`, or none for dark (src/lib/appearance.ts). */
const appearance = (b) => b.execute(() => ["light", "black"].find((c) => document.documentElement.classList.contains(c)) ?? "dark");
/** The Theme switch's option `name`. */
const themeOption = (b, name) => b.$(`//*[@role="radiogroup"][@aria-label="Theme"]//*[@role="radio"][contains(normalize-space(), ${JSON.stringify(name)})]`);
const section = async (b, name) => {
  await (await b.$(`//nav[@aria-label="Settings sections"]//button[contains(normalize-space(), ${JSON.stringify(name)})]`)).click();
  await sleep(500);
};
/** Waits until `read()` gives `want`, in window `label`; true when it did. */
const followed = async (b, label, read, want) => {
  await b.switchToWindow(label);
  return b
    .waitUntil(async () => (await read()) === want, { timeout: 10000, interval: 500 })
    .then(() => true)
    .catch(() => false);
};

let proc;
let b;
let shared = null;
let folder = null;
try {
  ({ proc, b } = await openApp());
  const before = await appearance(b);
  await goTo(b, "Settings");
  await switchToWindow(b, "settings");
  await r.step("window", "Settings opens in its own window", async () => {
    await b.$('nav[aria-label="Settings sections"]').waitForExist({ timeout: 20000 });
  });

  // Theme: Light (or Dark when already light), seen in both windows, then back.
  await section(b, "Appearance");
  const theme = before === "light" ? "Dark" : "Light";
  const want = theme.toLowerCase();
  await (await themeOption(b, theme)).click();
  r.check("theme", await followed(b, "settings", () => appearance(b), want), `${theme} applies to the Settings window`);
  r.check("theme-main", await followed(b, "main", () => appearance(b), want), `the main window follows to ${theme}`);
  await b.switchToWindow("settings");
  const back = before.charAt(0).toUpperCase() + before.slice(1);
  await (await themeOption(b, back)).click();
  r.check("theme-back", await followed(b, "main", () => appearance(b), before), `back to ${back} in both windows`);

  // Language: French, read in both windows' own words, then English again (the specs read English).
  await b.switchToWindow("settings");
  const language = await b.$('select[aria-label="Language"]');
  await language.selectByAttribute("value", "fr");
  const sections = () => b.$('nav[aria-label="Sections des réglages"]').isExisting();
  r.check("language", await followed(b, "settings", sections, true), "Français: the Settings window speaks French");
  const sidebar = async () =>
    /Vue d.ensemble/.test(
      await b
        .$("aside nav")
        .getText()
        .catch(() => ""),
    );
  r.check("language-main", await followed(b, "main", sidebar, true), "the main window's sidebar follows (Vue d'ensemble)");
  await b.switchToWindow("settings");
  await (await b.$('select[aria-label="Langue"], select[aria-label="Language"]')).selectByAttribute("value", "en");
  const english = async () =>
    /Overview/.test(
      await b
        .$("aside nav")
        .getText()
        .catch(() => ""),
    );
  r.check("language-back", await followed(b, "main", english, true), "back to English in the main window");

  // Shared folder: on, a folder picked through the picker, read back from the core.
  await b.switchToWindow("settings");
  await section(b, "Labs");
  shared = await invokeCmd(b, "shared_folder_get");
  folder = mkdtempSync(join(tmpdir(), "cyberctf-shared-"));
  await answerPickers(b, folder);
  const toggle = await b.$("#shared-folder-on");
  await toggle.waitForExist({ timeout: 20000 });
  if ((await toggle.getAttribute("aria-checked")) !== "true") {
    await toggle.click();
    await sleep(1500);
  }
  await (await b.$('//button[contains(normalize-space(), "Change")]')).click();
  await r.step("shared-folder", "a folder picked in Settings is the shared folder", async () => {
    await b.waitUntil(async () => (await invokeCmd(b, "shared_folder_get"))?.path === folder, { timeout: 15000, interval: 1000 });
    const shown = await b.$(`[title=${JSON.stringify(folder)}]`).isExisting();
    if (!shown) throw new Error("saved, but the row doesn't show the folder");
    return `shared at ${(await invokeCmd(b, "shared_folder_get")).mountPoint} in the box: ${folder}`;
  });

  // The attack VM's desktop: Desktop, then Headless (the default) again, each saved at once.
  await b.switchToWindow("settings");
  const desktop = (name) =>
    b.$(`//*[@role="radiogroup"][@aria-label="Attack VM desktop"]//*[@role="radio"][contains(normalize-space(), ${JSON.stringify(name)})]`);
  const stored = () => b.execute(() => localStorage.getItem("cyberctf.attackbox.desktop"));
  await r.step("attack-desktop", "Attack VM desktop: Desktop then Headless, each saved", async () => {
    await (await desktop("Desktop")).click();
    await b.waitUntil(async () => (await stored()) === "true", { timeout: 10000, timeoutMsg: "Desktop wasn't saved" });
    await (await desktop("Headless")).click();
    await b.waitUntil(async () => (await stored()) === "false", { timeout: 10000, timeoutMsg: "Headless wasn't saved" });
  });
} catch (e) {
  r.fail("error", e.message);
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  if (b && shared) {
    await b.switchToWindow("settings").catch(() => {});
    await invokeCmd(b, "shared_folder_set", { enabled: shared.enabled, path: shared.path }).catch((e) =>
      console.log("(couldn't put the shared folder back:", e.message + ")"),
    );
  }
  if (b) await closeWindow(b, "settings");
  if (folder) rmSync(folder, { recursive: true, force: true });
  await finish(b, proc);
}
r.exit();
