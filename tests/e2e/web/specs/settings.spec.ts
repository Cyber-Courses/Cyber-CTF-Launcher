import type { Page } from "@playwright/test";
import { test, expect } from "../fixtures";
import frSettings from "../../../../src/messages/fr/settings.json";
import jaSettings from "../../../../src/messages/ja/settings.json";

const html = (page: Page) => page.locator("html");

test.describe("settings", () => {
  test("a clean-up asks in the window's own dialog: no keeps everything, yes cleans, and a later question still asks (#383)", async ({
    page,
    settingsWindow,
  }) => {
    await settingsWindow.open("Maintenance");
    const cleanUp = page.getByRole("button", { name: "Clean up", exact: true });
    const dialog = page.getByRole("alertdialog", { name: /^Remove .* of lab downloads\?$/ });
    await cleanUp.click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByText(/^Freed /)).toHaveCount(0);
    // Asked again: the window's questions aren't all a no after the first.
    await cleanUp.click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Remove", exact: true }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByText(/^Freed /)).toBeVisible();
  });

  test("the theme switch changes the html class and is saved", async ({ page, settingsWindow }) => {
    await settingsWindow.open("Appearance");
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

  test("the language menu switches the UI language", async ({ page, settingsWindow }) => {
    await settingsWindow.open("Appearance");
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

  test("the shared-folder switch is there and toggles", async ({ page, settingsWindow }) => {
    await settingsWindow.open("Labs");
    const shared = page.getByRole("switch", { name: /Shared folder/ });
    await expect(shared).toBeVisible();
    await expect(shared).toHaveAttribute("aria-checked", "false");
    await shared.click();
    await expect(shared).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText("/Users/ada/cyberctf-share")).toBeVisible();
  });

  test("Remembered launch choices says one is kept, and Ask again forgets it", async ({ page, settingsWindow }) => {
    // A VM form remembered by "Where should the lab run?"'s "don't ask again".
    await page.addInitScript(() => {
      if (!sessionStorage.getItem("seeded")) {
        localStorage.setItem("cyberctf.devMock.settings", JSON.stringify({ vmForm: "vms" }));
        sessionStorage.setItem("seeded", "1");
      }
    });
    await settingsWindow.open("Labs");
    const askAgain = page.getByRole("button", { name: "Ask again", exact: true });
    // The row's words are its state: remembered, or asked each time.
    await expect(page.getByText("A container engine, VM form or hypervisor is remembered for this machine.")).toBeVisible();
    await expect(askAgain).toBeEnabled();

    await askAgain.click();
    await expect(page.getByText("The launch dialog asks each time.")).toBeVisible();
    await expect(askAgain).toBeDisabled();
    expect(
      await page.evaluate(() => (JSON.parse(localStorage.getItem("cyberctf.devMock.settings") ?? "{}") as { vmForm?: string | null }).vmForm ?? null),
    ).toBeNull();
    // Forgotten for good: a reload still asks.
    await page.reload();
    await settingsWindow.show("Labs");
    await expect(page.getByText("The launch dialog asks each time.")).toBeVisible();
  });

  test("About shows the app's version, this system and the install id", async ({ page, settingsWindow }) => {
    await settingsWindow.open("About");
    // Each value beside its label, as the mock's app reports them.
    for (const [label, value] of [
      ["Version", "v0.3.2"],
      ["System", "macos aarch64"],
      ["Install ID", "c0ffee"],
    ]) {
      await expect(page.getByText(label, { exact: true }).locator("xpath=..")).toContainText(value);
    }
  });

  test("Copy in About puts the diagnostics for a bug report on the clipboard, in English", async ({ page, context, settingsWindow }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    await settingsWindow.open("About");
    const diagnostics = page.getByRole("group", { name: "Diagnostics" });
    await diagnostics.getByRole("button", { name: "Copy", exact: true }).click();
    await expect(diagnostics.getByRole("button", { name: "Copied", exact: true })).toBeVisible();
    // The text is the contract with support: its English lines, whatever the UI's language.
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toContain("Cyber CTF v0.3.2");
    expect(copied).toContain("OS: macos aarch64");
    expect(copied).toMatch(/^Docker: running \(/m);
    expect(copied).toContain("VM provider: ");
    expect(copied).toContain("Install ID: c0ffee");
  });
});
