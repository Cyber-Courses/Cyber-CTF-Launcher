import { test, expect, LOCALES, openApp, navButton, pageTitle, rawKeys } from "./fixtures";
import en from "../src/messages/en/shell.json";
import fr from "../src/messages/fr/shell.json";
import es from "../src/messages/es/shell.json";
import de from "../src/messages/de/shell.json";
import ptBR from "../src/messages/pt-BR/shell.json";
import ja from "../src/messages/ja/shell.json";

const SHELL = { en, fr, es, de, "pt-BR": ptBR, ja };

// Every language: Overview and Labs render in it, with no raw message key on screen and no
// console error (the fixture also fails on a missing-translation warning).
for (const locale of LOCALES) {
  test.describe(`i18n ${locale}`, () => {
    test.use({ appLocale: locale });

    test("Overview and Labs render translated", async ({ page }) => {
      const tabs = SHELL[locale].tabs;
      await openApp(page);
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      await expect(navButton(page, tabs.home)).toHaveAttribute("aria-current", "page");
      for (const tab of [tabs.labs, tabs.machine, tabs.server, tabs.cloud]) await expect(navButton(page, tab)).toBeVisible();
      expect(await rawKeys(page), "raw keys on Overview").toEqual([]);

      await navButton(page, tabs.labs).click();
      await expect(pageTitle(page)).toHaveText(tabs.labs);
      await expect(
        page
          .locator("main")
          .getByRole("button", { name: /Invoice portal API/ })
          .first(),
      ).toBeVisible();
      expect(await rawKeys(page), "raw keys on Labs").toEqual([]);
    });
  });
}
