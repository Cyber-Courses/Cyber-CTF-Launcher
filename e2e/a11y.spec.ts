import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, animationsFinished, paletteKey } from "./fixtures";

// Accessibility, checked by axe on the main screens in both themes: text contrast (WCAG AA,
// 4.5:1 for the small mono and faint text the launcher uses a lot) and every rule axe rates
// critical (missing names on buttons and fields, broken ARIA, and the like). One test per screen,
// so a slow machine slows each a little instead of pushing one long walk past its time limit.

/** axe's findings that fail the test: any contrast issue, and anything critical. axe runs in the
 *  page itself (legacy mode): by default it finishes every run in a new blank tab, seconds per
 *  check on a loaded machine, only to reach cross-origin frames, which the launcher has none of. */
async function violations(page: Page) {
  const { violations } = await new AxeBuilder({ page }).setLegacyMode().analyze();
  return violations
    .filter((v) => v.id === "color-contrast" || v.impact === "critical")
    .flatMap((v) => v.nodes.map((n) => `${v.id} (${v.impact}): ${n.target.join(" ")} :: ${(n.any[0]?.message ?? n.failureSummary ?? "").slice(0, 160)}`));
}

/** The sidebar's screens, with their title once open. */
const SCREENS = [
  ["Overview", /Florian/],
  ["Labs", "Labs"],
  ["Machine", "Machine"],
  ["Servers", "Servers"],
  ["Cloud", "Cloud"],
] as const;

/** Opens the app on the page of the lab called `name`. */
async function openLab(page: Page, name: string) {
  await openApp(page);
  await navButton(page, "Labs").click();
  await page
    .locator("main")
    .getByRole("button", { name: `Open ${name}`, exact: true })
    .click();
}

for (const theme of ["dark", "light"] as const) {
  test.describe(`accessibility, ${theme} theme`, () => {
    test.beforeEach(async ({ context }) => {
      await context.addInitScript((t) => {
        try {
          localStorage.setItem("cyberctf.appearance", t);
        } catch {
          /* storage unavailable */
        }
      }, theme);
    });

    for (const [tab, heading] of SCREENS) {
      test(`the ${tab} screen passes axe's contrast and critical rules`, async ({ page }) => {
        await openApp(page);
        await navButton(page, tab).click();
        await expect(pageTitle(page)).toHaveText(heading);
        // Past the entrance animations (they start from a lighter state).
        await animationsFinished(page);
        expect(await violations(page)).toEqual([]);
      });
    }

    test("a running lab's page, with its diagram, passes too", async ({ page }) => {
      await openLab(page, "Invoice portal API");
      // The diagram fades in once laid out (it waits at opacity 0 till then, which no animation
      // shows): fully in, and the rest of the page settled.
      await expect(page.locator(".react-flow")).toHaveCSS("opacity", "1");
      await animationsFinished(page);
      expect(await violations(page)).toEqual([]);
    });

    test("the run dialog, over a stopped lab, passes too", async ({ page }) => {
      await openLab(page, "Blind orders");
      await page.getByRole("button", { name: /Start lab/ }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await animationsFinished(page);
      expect(await violations(page)).toEqual([]);
    });

    test("the command palette passes too", async ({ page }) => {
      await openApp(page);
      await page.keyboard.press(await paletteKey(page));
      await expect(page.getByRole("combobox")).toBeFocused();
      // Measured once its fade-in is over: mid-animation, its text reads as low contrast.
      await animationsFinished(page);
      expect(await violations(page)).toEqual([]);
    });

    test("the Settings window passes too", async ({ page }) => {
      await page.goto("/?window=settings&mock");
      const tabs = page.locator("nav button");
      await expect(tabs.first()).toBeVisible();
      const found: string[] = [];
      for (let i = 0; i < (await tabs.count()); i++) {
        await tabs.nth(i).click();
        await expect(tabs.nth(i)).toHaveAttribute("aria-current", "page");
        await animationsFinished(page);
        found.push(...(await violations(page)).map((v) => `settings tab ${i}: ${v}`));
      }
      expect(found).toEqual([]);
    });
  });
}
