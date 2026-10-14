import { AxeBuilder } from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// Accessibility, checked by axe on the main screens in both themes: text contrast (WCAG AA,
// 4.5:1 for the small mono and faint text the launcher uses a lot) and every rule axe rates
// critical (missing names on buttons and fields, broken ARIA, and the like).

/** axe's findings that fail the test: any contrast issue, and anything critical. */
async function violations(page: Page) {
  const { violations } = await new AxeBuilder({ page }).analyze();
  return violations
    .filter((v) => v.id === "color-contrast" || v.impact === "critical")
    .flatMap((v) => v.nodes.map((n) => `${v.id} (${v.impact}): ${n.target.join(" ")} :: ${(n.any[0]?.message ?? n.failureSummary ?? "").slice(0, 160)}`));
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

    test("the main screens pass axe's contrast and critical rules", async ({ page }) => {
      await openApp(page);
      const found: string[] = [];
      const check = async (where: string) => found.push(...(await violations(page)).map((v) => `${where}: ${v}`));

      for (const [tab, heading] of [
        ["Overview", /Florian/],
        ["Labs", "Labs"],
        ["Machine", "Machine"],
        ["Servers", "Servers"],
        ["Cloud", "Cloud"],
      ] as const) {
        await navButton(page, tab).click();
        await expect(pageTitle(page)).toHaveText(heading);
        // Past the entrance animations (they start from a lighter state).
        await page.waitForTimeout(600);
        await check(tab);
      }

      // A running lab's page, with its diagram.
      await navButton(page, "Labs").click();
      await page.locator("main").getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
      await expect(page.locator(".react-flow")).toBeVisible();
      await page.waitForTimeout(600);
      await check("lab page");

      // The run dialog, over a stopped lab.
      await navButton(page, "Labs").click();
      await page.locator("main").getByRole("button", { name: "Open Blind orders", exact: true }).click();
      await page.getByRole("button", { name: /Start lab/ }).click();
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.waitForTimeout(400);
      await check("run dialog");
      await page.keyboard.press("Escape");

      // The command palette.
      await page.keyboard.press("Control+k");
      await expect(page.getByRole("combobox")).toBeFocused();
      // Measured once its fade-in is over: mid-animation, its text reads as low contrast.
      // (Looping animations, like a spinner's, never finish: not waited for.)
      await page.evaluate(() =>
        Promise.all(
          document
            .getAnimations()
            .filter((a) => a.effect?.getTiming().iterations !== Infinity)
            .map((a) => a.finished.catch(() => {})),
        ),
      );
      await check("palette");

      expect(found).toEqual([]);
    });

    test("the Settings window passes too", async ({ page }) => {
      await page.goto("/?window=settings&mock");
      const tabs = page.locator("nav button");
      await expect(tabs.first()).toBeVisible();
      const found: string[] = [];
      for (let i = 0; i < (await tabs.count()); i++) {
        await tabs.nth(i).click();
        await page.waitForTimeout(500);
        found.push(...(await violations(page)).map((v) => `settings tab ${i}: ${v}`));
      }
      expect(found).toEqual([]);
    });
  });
}
