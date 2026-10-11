import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, breadcrumb } from "./fixtures";

// The mock's catalogue (src/lib/dev-mock.ts): Invoice portal API runs, Public backup is solved.
const ALL = ["Invoice portal API", "GOAD Light", "Blind orders", "Exposed dashboard", "Public backup", "EC2 SSRF"];

/** The lab rows the list shows, by title. */
const row = (page: Page, title: string) => page.locator("main").getByRole("button", { name: `Open ${title}`, exact: true });

async function expectListed(page: Page, shown: string[]) {
  for (const title of ALL) {
    if (shown.includes(title)) await expect(row(page, title), `${title} listed`).toBeVisible();
    else await expect(row(page, title), `${title} filtered out`).toHaveCount(0);
  }
}

const filter = (page: Page, group: string, option: string) => page.getByRole("radiogroup", { name: group }).getByRole("radio", { name: option, exact: true });

test.describe("labs", () => {
  test.beforeEach(async ({ page }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await expect(pageTitle(page)).toHaveText("Labs");
  });

  test("the list shows the catalogue", async ({ page }) => {
    await expectListed(page, ALL);
    await expect(navButton(page, "Labs")).toContainText("6");
  });

  test("filters narrow the list", async ({ page }) => {
    await filter(page, "Status", "Running").click();
    await expect(filter(page, "Status", "Running")).toHaveAttribute("aria-checked", "true");
    await expectListed(page, ["Invoice portal API"]);

    await filter(page, "Status", "Solved").click();
    await expectListed(page, ["Public backup"]);

    await filter(page, "Status", "To do").click();
    await expectListed(page, ["GOAD Light", "Blind orders", "Exposed dashboard", "EC2 SSRF"]);
    await filter(page, "Status", "All").click();

    await filter(page, "Runtime", "VM").click();
    await expectListed(page, ["GOAD Light"]);
    await filter(page, "Runtime", "Cloud").click();
    await expectListed(page, ["Invoice portal API", "Public backup", "EC2 SSRF"]);
    await filter(page, "Runtime", "Any runtime").click();

    await filter(page, "Level", "Hard").click();
    await expectListed(page, ["GOAD Light"]);
    await filter(page, "Level", "Medium").click();
    await expectListed(page, ["Blind orders", "Exposed dashboard", "EC2 SSRF"]);
    await filter(page, "Level", "Any level").click();

    // The search matches titles, categories, descriptions and skills.
    const search = page.getByRole("textbox", { name: "Search labs and skills" });
    await search.fill("bucket");
    await expectListed(page, ["Public backup"]);
    await search.fill("kerberoasting");
    await expectListed(page, ["GOAD Light"]);
    await search.fill("web");
    await expectListed(page, ["Invoice portal API", "Blind orders"]);
    await search.fill("");
    await expectListed(page, ALL);
  });

  test("a running lab shows its network with the machines and the attack box", async ({ page }) => {
    await row(page, "Invoice portal API").click();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");
    await expect(breadcrumb(page)).toHaveText("Invoice portal API");

    const diagram = page.locator(".react-flow");
    await expect(diagram).toBeVisible();
    await expect(diagram).toContainText("10.42.0.0/24");
    for (const [machine, ip] of [
      ["api", "10.42.0.3"],
      ["postgres", "10.42.0.4"],
      ["cache", "10.42.0.5"],
    ]) {
      const node = diagram.locator(".react-flow__node").filter({ hasText: ip });
      await expect(node, machine).toBeVisible();
      await expect(node).toContainText(machine);
    }
    const attackBox = diagram.locator(".react-flow__node").filter({ hasText: "Attack box" });
    await expect(attackBox).toBeVisible();
    await expect(attackBox).toContainText("10.42.0.9");
  });

  test("Stop & remove asks first, and Cancel keeps the lab", async ({ page }) => {
    await row(page, "Invoice portal API").click();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");

    await page.getByRole("button", { name: "Stop & remove" }).click();
    const dialog = page.getByRole("alertdialog", { name: "Remove Invoice portal API?" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Remove" })).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");
    await expect(page.locator(".react-flow")).toBeVisible();
  });

  test("Escape on a lab page goes back to the list", async ({ page }) => {
    await row(page, "Blind orders").click();
    await expect(pageTitle(page)).toHaveText("Blind orders");
    await page.locator("main h1").click();
    await page.keyboard.press("Escape");
    await expect(pageTitle(page)).toHaveText("Labs");
  });

  test("Escape closing the command palette stays on the lab page", async ({ page }) => {
    await row(page, "Blind orders").click();
    await expect(pageTitle(page)).toHaveText("Blind orders");
    await page.keyboard.press("ControlOrMeta+k");
    const palette = page.getByRole("dialog", { name: "Command menu" });
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toBeHidden();
    await expect(pageTitle(page)).toHaveText("Blind orders");
  });
});
