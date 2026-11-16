import { test, expect, openApp } from "../fixtures";

// What a lab's page says beside its start and its network, through the dev mock's catalogue
// (src/lib/dev-mock/labs.ts): what the lab needs of a machine (`lab_requirements`) and the
// observers it puts beside itself (`lab_tools`: Burp Suite, on the running Invoice portal API).

test.describe("lab page", () => {
  test("a lab's needs are said before it starts: a range over its forms, or one value", async ({ page, app, labs }) => {
    await openApp(page, "&deploying=none");
    // GOAD Light: its lightest form to its heaviest. The figures are the contract, as formatted.
    await labs.openLab("GOAD Light");
    const needs = app.main.getByText("Needs", { exact: true }).locator("xpath=..");
    await expect(needs).toContainText("4–6 vCPU · 2.7–13 GB RAM · 25–120 GB disk");
    // Blind orders: one form, one value each.
    await labs.openLab("Blind orders");
    await expect(app.main.getByText("Needs", { exact: true }).locator("xpath=..")).toContainText("2 vCPU · 1.1 GB RAM · 20 GB disk");
  });

  test("the observers a running lab puts beside itself are listed at their address", async ({ page, app, labs }) => {
    await openApp(page);
    await labs.openLab("Invoice portal API");
    // The panel's title, with how many there are beside it.
    const observers = app.main.getByRole("heading", { name: "Observers", exact: true });
    await expect(observers).toBeVisible();
    await expect(observers.locator("xpath=..")).toContainText("Observers1");
    await expect(app.main.getByText("Burp Suite", { exact: true })).toBeVisible();
    await expect(app.main.getByText("default 10.42.0.9", { exact: true })).toBeVisible();
  });
});
