import { test, expect, openApp, horizontalOverflow } from "./fixtures";

// At 1280x800 (the config's viewport), no main screen scrolls sideways.
test.describe("layout", () => {
  test("main screens fit the width", async ({ page, app, labs, lab }) => {
    expect(page.viewportSize()).toEqual({ width: 1280, height: 800 });
    await openApp(page);
    for (const [tab, heading] of [
      ["Overview", /Ada/],
      ["Labs", "Labs"],
      ["Machine", "Machine"],
      ["Servers", "Servers"],
      ["Cloud", "Cloud"],
    ] as const) {
      await app.nav(tab).click();
      await expect(app.title).toHaveText(heading);
      expect(await horizontalOverflow(page), `${tab} overflows`).toEqual([]);
    }

    // A running lab's page, with its network diagram.
    await app.nav("Labs").click();
    await labs.open("Invoice portal API").click();
    await expect(lab.diagram).toBeVisible();
    expect(await horizontalOverflow(page), "lab page overflows").toEqual([]);
  });

  // A wide window: the shell spans it, the list screens stop at 90rem (centred), and a lab's
  // diagram leaves the main column for the page's width, drawing the mock's wide lab (three
  // networks, two pivots) whole, without scrolling sideways.
  test("a wide window gives a lab's diagram the page's width", async ({ page, app, labs, lab }) => {
    await page.setViewportSize({ width: 1920, height: 1080 });
    await openApp(page);
    for (const [tab, heading] of [
      ["Overview", /Ada/],
      ["Labs", "Labs"],
      ["Machine", "Machine"],
      ["Servers", "Servers"],
    ] as const) {
      await app.nav(tab).click();
      await expect(app.title).toHaveText(heading);
      const width = await app.title.evaluate((h) => h.closest(".max-w-\\[90rem\\]")?.getBoundingClientRect().width);
      expect(width, `${tab} width`).toBe(1440);
      expect(await horizontalOverflow(page), `${tab} overflows`).toEqual([]);
    }

    await app.nav("Labs").click();
    await labs.open("Invoice portal API").click();
    const shell = page.locator(".topology-shell");
    await expect(lab.diagramNode("10.42.2.21")).toBeVisible();
    const [card, rail] = await Promise.all([page.locator(".hostcard").boundingBox(), page.locator("aside.h-fit").boundingBox()]);
    // Above the rail, as wide as the main column and the rail together.
    expect(card!.y + card!.height).toBeLessThan(rail!.y);
    expect(card!.x + card!.width).toBeCloseTo(rail!.x + rail!.width, 0);
    await expect(shell).not.toHaveAttribute("data-more-right");
    expect(await horizontalOverflow(page), "lab page overflows").toEqual([]);

    // Back to a narrow window: the diagram returns beside the rail, and scrolls sideways.
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(shell).toHaveAttribute("data-more-right", "true");
    const narrow = await page.locator(".hostcard").boundingBox();
    const railNow = await page.locator("aside.h-fit").boundingBox();
    expect(narrow!.x + narrow!.width).toBeLessThan(railNow!.x);
  });

  test("Settings fits the width", async ({ page }) => {
    await page.goto("/?mock&window=settings");
    const sections = page.getByRole("navigation", { name: "Settings sections" });
    for (const section of ["Account", "Appearance", "Labs", "Maintenance", "About"]) {
      await sections.getByRole("button", { name: section }).click();
      await expect(page.getByRole("heading", { level: 2 }).first()).toBeVisible();
      expect(await horizontalOverflow(page), `${section} overflows`).toEqual([]);
    }
  });
});
