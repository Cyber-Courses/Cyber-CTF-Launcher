import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";
import frSettings from "../../src/messages/fr/settings.json";
import frShell from "../../src/messages/fr/shell.json";

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

  test("the lab ports picked in Settings are what a start uses, without asking", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    await sections(page).getByRole("button", { name: "Labs" }).click();
    await radio(page, "Lab ports", "Default").click();

    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Exposed dashboard", exact: true }).click();
    await page.getByRole("button", { name: "Start lab" }).click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    // Containers here: Settings has answered which host ports, so picking them asks nothing more.
    const containers = where.getByRole("radiogroup", { name: "This machine" }).getByRole("radio", { name: /^Container/ });
    await containers.click();
    await expect(containers).toHaveAttribute("aria-checked", "true");
    await expect(where.getByRole("radiogroup", { name: "Host ports" })).toHaveCount(0);
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(where).toBeHidden();
    // Its service is published on this machine on its own port, not a random one.
    await expect(page.getByRole("button", { name: "Stop & remove" })).toBeEnabled();
    await expect(page.locator("main").getByRole("button", { name: ":8443", exact: true })).toBeVisible();
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
  test("a fresh Windows machine lists what to set up", async ({ page, context }) => {
    await openApp(page, "&machine=fresh&os=windows&arch=x86_64");
    await navButton(page, "Machine").click();
    await expect(page.locator("main")).toContainText("2 lab types need setup");
    await expect(page.locator("main")).toContainText("Windows x86_64");
    const empty = page.getByRole("region", { name: "No provider installed" });
    await expect(empty).toContainText("Install a container engine or a hypervisor to run labs on this machine.");
    // Add a provider: what Windows can run, each one to install.
    const [add] = await Promise.all([context.waitForEvent("page"), empty.getByRole("button", { name: "Add a provider" }).click()]);
    await expect(add.getByRole("heading", { level: 1, name: "Add a provider" })).toBeVisible();
    await expect(add.getByRole("button", { name: "Install Docker Desktop" })).toBeVisible();
    for (const hypervisor of ["Install VirtualBox", "Get VMware Workstation", "Install Hyper-V"]) {
      await expect(add.getByRole("button", { name: hypervisor })).toBeVisible();
    }
  });
});
