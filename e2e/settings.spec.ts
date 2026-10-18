import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import frSettings from "../src/messages/fr/settings.json";
import jaSettings from "../src/messages/ja/settings.json";

const html = (page: Page) => page.locator("html");

/** The Settings window, as the app opens it (`open_settings`), on one of its sections. */
async function openSettings(page: Page, section: string) {
  await page.goto("/?mock&window=settings");
  const sections = page.getByRole("navigation", { name: "Settings sections" });
  await sections.getByRole("button", { name: section }).click();
  await expect(sections.getByRole("button", { name: section })).toHaveAttribute("aria-current", "page");
}

test.describe("settings", () => {
  test("the theme switch changes the html class and is saved", async ({ page }) => {
    await openSettings(page, "Appearance");
    const theme = page.getByRole("radiogroup", { name: "Theme" });
    await expect(theme.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    await expect(html(page)).not.toHaveClass(/\b(light|black)\b/);

    await theme.getByRole("radio", { name: "Light" }).click();
    await expect(html(page)).toHaveClass(/\blight\b/);
    expect(await page.evaluate(() => localStorage.getItem("cyberctf.appearance"))).toBe("light");

    await theme.getByRole("radio", { name: "Black" }).click();
    await expect(html(page)).toHaveClass(/\bblack\b/);
    await expect(html(page)).not.toHaveClass(/\blight\b/);

    await theme.getByRole("radio", { name: "Dark" }).click();
    await expect(html(page)).not.toHaveClass(/\b(light|black)\b/);
  });

  test("the language menu switches the UI language", async ({ page }) => {
    await openSettings(page, "Appearance");
    await page.getByRole("combobox", { name: "Language" }).selectOption("fr");
    await expect(html(page)).toHaveAttribute("lang", "fr");
    await expect(page.getByRole("heading", { level: 2, name: frSettings.appearance.title })).toBeVisible();
    await expect(page.getByRole("navigation", { name: frSettings.tabs.label })).toContainText(frSettings.tabs.maintenance);
    expect(await page.evaluate(() => localStorage.getItem("cyberctf.locale"))).toBe("fr");

    // The menu is labelled in the new language now.
    await page.getByRole("combobox", { name: frSettings.appearance.language }).selectOption("ja");
    await expect(html(page)).toHaveAttribute("lang", "ja");
    await expect(page.getByRole("heading", { level: 2, name: jaSettings.appearance.title })).toBeVisible();

    await page.getByRole("combobox", { name: jaSettings.appearance.language }).selectOption("en");
    await expect(page.getByRole("heading", { level: 2, name: "Appearance" })).toBeVisible();
  });

  test("the shared-folder switch is there and toggles", async ({ page }) => {
    await openSettings(page, "Labs");
    const shared = page.getByRole("switch", { name: /Shared folder/ });
    await expect(shared).toBeVisible();
    await expect(shared).toHaveAttribute("aria-checked", "false");
    await shared.click();
    await expect(shared).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText("/Users/florian/cyberctf-share")).toBeVisible();
  });
});
