import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";
import frSettings from "../src/messages/fr/settings.json";
import frShell from "../src/messages/fr/shell.json";

// Settings are kept on this machine (localStorage): each one picked in the Settings window is
// still there after a reload, and the main window starts with it.

const sections = (page: Page) => page.getByRole("navigation", { name: /^(Settings sections|Sections des réglages)$/ });
const radio = (page: Page, group: string, option: string | RegExp) => page.getByRole("radiogroup", { name: group }).getByRole("radio", { name: option });

test.describe("settings persistence", () => {
  test("theme, language, lab ports and attack box image survive a reload", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    await sections(page).getByRole("button", { name: "Labs" }).click();
    await radio(page, "Lab ports", "Default").click();
    await radio(page, "Attack box image", /^Kali/).click();
    await expect(radio(page, "Attack box image", /^Kali/)).toHaveAttribute("aria-checked", "true");
    await sections(page).getByRole("button", { name: "Appearance" }).click();
    await radio(page, "Theme", "Light").click();
    await page.getByRole("combobox", { name: "Language" }).selectOption("fr");
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");

    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");
    await expect(page.locator("html")).toHaveClass(/\blight\b/);
    await sections(page).getByRole("button", { name: frSettings.tabs.appearance }).click();
    await expect(radio(page, frSettings.appearance.theme, frSettings.appearance.themes.light)).toHaveAttribute("aria-checked", "true");
    await expect(page.getByRole("combobox", { name: frSettings.appearance.language })).toHaveValue("fr");
    await sections(page).getByRole("button", { name: frSettings.tabs.labs }).click();
    await expect(radio(page, frSettings.labPorts.title, frSettings.labPorts.options.default)).toHaveAttribute("aria-checked", "true");
    await expect(radio(page, frSettings.attackBox.title, /^Kali/)).toHaveAttribute("aria-checked", "true");
    await expect(radio(page, frSettings.attackBox.title, /^Cyber CTF/)).toHaveAttribute("aria-checked", "false");

    // The main window opens in them too.
    await page.goto("/?mock");
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");
    await expect(page.locator("html")).toHaveClass(/\blight\b/);
    await expect(navButton(page, frShell.tabs.labs)).toBeVisible();
    await navButton(page, frShell.tabs.machine).click();
    await expect(pageTitle(page)).toHaveText(frShell.tabs.machine);
  });

  test("the lab ports picked in Settings are the start dialog's default", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    await sections(page).getByRole("button", { name: "Labs" }).click();
    await radio(page, "Lab ports", "Default").click();

    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Exposed dashboard", exact: true }).click();
    await page.getByRole("button", { name: "Start lab" }).click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: /^The lab's default ports/ })).toHaveAttribute("aria-checked", "true");
    await expect(where.getByText(/already uses one|collide|in use/i).first()).toBeVisible();
    await where.getByRole("button", { name: "Cancel" }).click();
    await expect(where).toBeHidden();
  });

  // BUG (accessibility): each Settings section keeps a "Saved" mark in the page at all times, only
  // faded out (opacity 0), inside an aria-live region. A screen reader reads "Saved" next to every
  // section title before anything was changed, and the live region never announces a save: its
  // text doesn't change, only its opacity. Fix (settings-layout.tsx, SectionTitle): render the text
  // only while `saved` (keep the empty live region), or hide it with aria-hidden when not saved.
  test("the Saved mark is only there after a change", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    await sections(page).getByRole("button", { name: "Labs" }).click();
    const title = page.locator("section").filter({ has: page.getByRole("heading", { level: 2, name: "Labs" }) });
    // The section's description says "Saved on this computer" too: the mark is one more "Saved".
    const saves = async () => ((await title.ariaSnapshot()).match(/\bSaved\b/g) ?? []).length;
    const before = await saves();
    await radio(page, "Lab ports", "Random").click();
    await expect.poll(saves).toBe(before + 1);
  });
});

test.describe("machine page", () => {
  test("the Health and Setup views switch, and the setup checklist says what is done", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Machine").click();
    const view = page.getByRole("radiogroup", { name: "Machine view" });
    await expect(view.getByRole("radio", { name: "Health" })).toHaveAttribute("aria-checked", "true");
    await expect(page.locator("main").getByRole("heading", { level: 3, name: "Engines" })).toBeVisible();
    await expect(page.locator("main")).toContainText("Ready for labs");
    await expect(page.locator("main")).toContainText("macOS arm64");

    await view.getByRole("radio", { name: "Setup" }).click();
    await expect(view.getByRole("radio", { name: "Setup" })).toHaveAttribute("aria-checked", "true");
    await expect(page.locator("main").getByRole("heading", { level: 3, name: "Guided setup" })).toBeVisible();
    await expect(page.locator("main").getByText(/^\d+ of \d+ done$/)).toBeVisible();
    await expect(page.locator("main").getByRole("heading", { level: 3, name: "Engines" })).toHaveCount(0);

    // Keyboard: the arrow keys move between the two views.
    await view.getByRole("radio", { name: "Setup" }).focus();
    await page.keyboard.press("ArrowLeft");
    await expect(view.getByRole("radio", { name: "Health" })).toHaveAttribute("aria-checked", "true");
    await expect(page.locator("main").getByRole("heading", { level: 3, name: "Engines" })).toBeVisible();
  });

  test("a fresh Windows machine lists what to set up", async ({ page }) => {
    await openApp(page, "&machine=fresh&os=windows&arch=x86_64");
    await navButton(page, "Machine").click();
    await expect(page.locator("main")).toContainText("2 lab types need setup");
    await expect(page.locator("main")).toContainText("Windows x86_64");
    await page.getByRole("radiogroup", { name: "Machine view" }).getByRole("radio", { name: "Setup" }).click();
    await expect(page.locator("main").getByText("1 of 8 done", { exact: true })).toBeVisible();
    await expect(page.locator("main").getByText("not installed").first()).toBeVisible();
  });
});
