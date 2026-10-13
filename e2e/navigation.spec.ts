import { test, expect, openApp, navButton, pageTitle, breadcrumb } from "./fixtures";

test.describe("navigation", () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test("every sidebar screen opens with its heading", async ({ page }) => {
    for (const [tab, heading] of [
      ["Labs", "Labs"],
      ["Machine", "Machine"],
      ["Servers", "Servers"],
      ["Cloud", "Cloud"],
      ["Overview", /Florian/],
    ] as const) {
      await navButton(page, tab).click();
      await expect(navButton(page, tab)).toHaveAttribute("aria-current", "page");
      await expect(pageTitle(page)).toHaveText(heading);
      // The breadcrumb ends with the screen's name.
      await expect(breadcrumb(page)).toHaveText(tab);
    }
  });

  test("Settings opens in its own window", async ({ page, context }) => {
    const [settings] = await Promise.all([
      context.waitForEvent("page"),
      page
        .locator("aside")
        .getByRole("button", { name: /^Settings/ })
        .click(),
    ]);
    await expect(settings).toHaveURL(/window=settings/);
    await expect(settings.getByRole("navigation", { name: "Settings sections" })).toBeVisible();
    await expect(settings.getByRole("heading", { level: 2, name: "Account" })).toBeVisible();
  });

  test("command palette opens and navigates", async ({ page }) => {
    await page.keyboard.press("ControlOrMeta+k");
    const palette = page.getByRole("dialog", { name: "Command menu" });
    await expect(palette).toBeVisible();
    await expect(palette.getByPlaceholder("Search labs, screens, actions…")).toBeFocused();

    await page.keyboard.type("servers");
    // A combobox over a listbox: the field names the active option.
    const option = palette.getByRole("option", { name: "Go to Servers" });
    await expect(option).toBeVisible();
    await expect(option).toHaveAttribute("aria-selected", "true");
    await expect(palette.getByRole("combobox")).toHaveAttribute("aria-activedescendant", (await option.getAttribute("id"))!);
    await page.keyboard.press("Enter");
    await expect(palette).toBeHidden();
    await expect(pageTitle(page)).toHaveText("Servers");

    // Escape closes it without doing anything.
    await page.keyboard.press("ControlOrMeta+k");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toBeHidden();
    await expect(pageTitle(page)).toHaveText("Servers");

    // A lab, picked by name from the sidebar's Find button, opens its page.
    await page.getByRole("button", { name: /^Find/ }).click();
    await expect(palette).toBeVisible();
    await page.keyboard.type("GOAD");
    await palette.getByRole("option", { name: /GOAD Light/ }).click();
    await expect(palette).toBeHidden();
    await expect(pageTitle(page)).toHaveText("GOAD Light");
  });
});
