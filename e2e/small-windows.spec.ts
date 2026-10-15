import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, horizontalOverflow, animationsFinished } from "./fixtures";
import deSettings from "../src/messages/de/settings.json";

// The windows at their smallest (src-tauri: Settings 560x480, server setup 560x560) and a lab
// wider than its card: nothing folds into a column a word wide, and the primary buttons stay in
// view.

const box = async (page: Page, locator: ReturnType<Page["locator"]>) => (await locator.boundingBox())!;

test.describe("Settings in German at its minimum size", () => {
  test.use({ appLocale: "de", viewport: { width: 560, height: 480 } });

  test("the side menu becomes a tab bar and the labels keep their width", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    const sections = page.getByRole("navigation", { name: deSettings.tabs.label });
    const tabs = sections.getByRole("button");
    await expect(tabs).toHaveCount(5);
    // One row of tabs above the section, not a rail beside it.
    const ys = await tabs.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().y)));
    expect(new Set(ys).size).toBe(1);
    await sections.getByRole("button", { name: deSettings.tabs.labs }).click();
    const heading = page.getByRole("heading", { level: 2, name: deSettings.labs.title });
    expect((await box(page, heading)).y).toBeGreaterThan((await box(page, sections)).y + 20);

    // The Lab ports label beside its control, at a readable width (it was one word per line).
    const label = page.getByText(deSettings.labPorts.title, { exact: true });
    expect((await box(page, label)).width).toBeGreaterThan(180);
    expect(await horizontalOverflow(page)).toEqual([]);
  });

  test("a row narrower than 420px puts its label above its control", async ({ page }) => {
    // Below the window's minimum, to reach the rows' own breakpoint.
    await page.setViewportSize({ width: 440, height: 600 });
    await page.goto("/?mock&window=settings");
    await page.getByRole("navigation", { name: deSettings.tabs.label }).getByRole("button", { name: deSettings.tabs.labs }).click();
    const label = await box(page, page.getByText(deSettings.labPorts.title, { exact: true }));
    const control = await box(page, page.getByRole("radiogroup", { name: deSettings.labPorts.title }));
    expect(control.y).toBeGreaterThanOrEqual(label.y + label.height);
    expect(Math.abs(control.x - label.x)).toBeLessThan(2);
    expect(await horizontalOverflow(page)).toEqual([]);
  });
});

test.describe("the server setup window at its minimum size", () => {
  test.use({ viewport: { width: 560, height: 560 } });

  for (const [kind, url] of [
    ["a server", "/server-setup?mock"],
    ["a cloud account", "/server-setup?kind=cloud&mock"],
  ] as const) {
    test(`keeps Continue in view for ${kind}`, async ({ page }) => {
      await page.goto(url);
      const next = page.getByRole("button", { name: "Continue", exact: true });
      await expect(next).toBeInViewport({ ratio: 1 });
      // The next step too: its Continue sits in the same place (once the step's rise-in ends).
      await animationsFinished(page);
      const at = (await box(page, next)).y;
      await next.click();
      await expect(page.getByText("Step 2 of", { exact: false })).toBeVisible();
      await expect(next).toBeInViewport({ ratio: 1 });
      await animationsFinished(page);
      expect(Math.abs((await box(page, next)).y - at)).toBeLessThanOrEqual(1);
    });
  }
});

test.describe("a lab wider than its card", () => {
  const openWideLab = async (page: Page) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    await expect(page.locator(".react-flow__node").filter({ hasText: "10.42.2.21" })).toBeAttached();
  };

  test("says there is more, and fits to the width on request, remembered", async ({ page }) => {
    await openWideLab(page);
    const shell = page.locator(".topology-shell");
    const fit = page.getByRole("button", { name: "Fit to width" });
    await expect(shell).toHaveAttribute("data-more-right", "true");
    await expect(page.getByRole("button", { name: "Scroll for more" })).toBeVisible();
    await expect(fit).toHaveAttribute("aria-pressed", "false");

    await fit.click();
    await expect(fit).toHaveAttribute("aria-pressed", "true");
    await expect(shell).not.toHaveAttribute("data-more-right");
    await expect(page.getByRole("button", { name: "Scroll for more" })).toHaveCount(0);
    const [canvas, card] = await Promise.all([page.locator(".topology-canvas").boundingBox(), shell.boundingBox()]);
    expect(canvas!.width).toBeCloseTo(card!.width, 0);
    // The whole lab is in the card: its last machine too.
    await expect(page.locator(".react-flow__node").filter({ hasText: "10.42.2.21" })).toBeInViewport();

    await page.reload();
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    await expect(page.getByRole("button", { name: "Fit to width" })).toHaveAttribute("aria-pressed", "true");
  });

  test("offers nothing in a window the lab already fits", async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await openWideLab(page);
    await expect(page.locator(".topology-shell")).not.toHaveAttribute("data-more-right");
    await expect(page.getByRole("button", { name: "Fit to width" })).toHaveCount(0);
  });
});
