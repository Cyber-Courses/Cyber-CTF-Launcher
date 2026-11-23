import type { Page } from "@playwright/test";
import { test, expect, openApp, paletteKey, answerConfirm } from "../fixtures";
import type { LabsScreen } from "../pages/labs";

// The mock's catalogue (src/lib/dev-mock/labs.ts): Invoice portal API runs, Public backup is solved.
const ALL = ["Invoice portal API", "GOAD Light", "Blind orders", "Exposed dashboard", "Public backup", "EC2 SSRF"];

async function expectListed(labs: LabsScreen, shown: string[]) {
  for (const title of ALL) {
    if (shown.includes(title)) await expect(labs.open(title), `${title} listed`).toBeVisible();
    else await expect(labs.open(title), `${title} filtered out`).toHaveCount(0);
  }
}

const filter = (page: Page, group: string, option: string) => page.getByRole("radiogroup", { name: group }).getByRole("radio", { name: option, exact: true });

test.describe("labs", () => {
  test.beforeEach(async ({ page, app }) => {
    await openApp(page);
    await app.nav("Labs").click();
    await expect(app.title).toHaveText("Labs");
  });

  test("the list shows the catalogue", async ({ app, labs }) => {
    await expectListed(labs, ALL);
    await expect(app.nav("Labs")).toContainText("6");
  });

  test("filters narrow the list", async ({ page, labs }) => {
    await filter(page, "Status", "Running").click();
    await expect(filter(page, "Status", "Running")).toHaveAttribute("aria-checked", "true");
    await expectListed(labs, ["Invoice portal API"]);

    await filter(page, "Status", "Solved").click();
    await expectListed(labs, ["Public backup"]);

    await filter(page, "Status", "To do").click();
    await expectListed(labs, ["GOAD Light", "Blind orders", "Exposed dashboard", "EC2 SSRF"]);
    await filter(page, "Status", "All").click();

    await filter(page, "Runtime", "VM").click();
    await expectListed(labs, ["GOAD Light"]);
    await filter(page, "Runtime", "Cloud").click();
    await expectListed(labs, ["Invoice portal API", "Public backup", "EC2 SSRF"]);
    await filter(page, "Runtime", "Any runtime").click();

    await filter(page, "Level", "Hard").click();
    await expectListed(labs, ["GOAD Light"]);
    await filter(page, "Level", "Medium").click();
    await expectListed(labs, ["Blind orders", "Exposed dashboard", "EC2 SSRF"]);
    await filter(page, "Level", "Any level").click();

    // The search matches titles, categories, descriptions and skills.
    const search = labs.search;
    await search.fill("bucket");
    await expectListed(labs, ["Public backup"]);
    await search.fill("kerberoasting");
    await expectListed(labs, ["GOAD Light"]);
    await search.fill("web");
    await expectListed(labs, ["Invoice portal API", "Blind orders"]);
    await search.fill("");
    await expectListed(labs, ALL);
  });

  test("a running lab shows its network with the machines and the attack box", async ({ app, lab, labs }) => {
    await labs.open("Invoice portal API").click();
    await expect(app.title).toHaveText("Invoice portal API");
    await expect(app.breadcrumb).toHaveText("Invoice portal API");

    const diagram = lab.diagram;
    await expect(diagram).toBeVisible();
    await expect(diagram).toContainText("10.42.0.0/24");
    for (const [machine, ip] of [
      ["api", "10.42.0.3"],
      ["postgres", "10.42.0.4"],
      ["cache", "10.42.0.5"],
    ]) {
      const node = lab.diagramNode(ip);
      await expect(node, machine).toBeVisible();
      await expect(node).toContainText(machine);
    }
    const attackBox = lab.diagramNode("Attack box");
    await expect(attackBox).toBeVisible();
    await expect(attackBox).toContainText("10.42.0.9");
  });

  test("Stop & remove asks first, and Cancel keeps the lab", async ({ page, app, lab, labs }) => {
    await labs.open("Invoice portal API").click();
    await expect(app.title).toHaveText("Invoice portal API");

    const asked = answerConfirm(page, false);
    await page.getByRole("button", { name: "Stop & remove" }).click();
    expect(await asked).toContain("Remove Invoice portal API?");
    await expect(app.title).toHaveText("Invoice portal API");
    await expect(lab.diagram).toBeVisible();
  });

  test("Escape on a lab page goes back to the list", async ({ page, app, labs }) => {
    await labs.open("Blind orders").click();
    await expect(app.title).toHaveText("Blind orders");
    await app.title.click();
    await page.keyboard.press("Escape");
    await expect(app.title).toHaveText("Labs");
  });

  test("Escape closing the command palette stays on the lab page", async ({ page, app, labs }) => {
    await labs.open("Blind orders").click();
    await expect(app.title).toHaveText("Blind orders");
    await page.keyboard.press(await paletteKey(page));
    const palette = page.getByRole("dialog", { name: "Command menu" });
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toBeHidden();
    await expect(app.title).toHaveText("Blind orders");
  });
});
