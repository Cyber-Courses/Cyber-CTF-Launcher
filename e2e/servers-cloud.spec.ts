import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

test.describe("servers and cloud", () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test("Servers lists the hosts", async ({ page }) => {
    await navButton(page, "Servers").click();
    await expect(pageTitle(page)).toHaveText("Servers");
    const main = page.locator("main");
    await expect(main.getByText("homelab-pve", { exact: true })).toBeVisible();
    await expect(main.getByText("office-esxi", { exact: true })).toBeVisible();
    await expect(main).toContainText("192.168.1.20");
    // Both hosts answer the mock's test.
    await expect(main.getByText("2 of 2 online", { exact: false })).toBeVisible();
    // Cloud accounts live on the Cloud screen, not here.
    await expect(main.getByText("AWS sandbox")).toHaveCount(0);
  });

  test("Add server opens the setup", async ({ page, context }) => {
    await navButton(page, "Servers").click();
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Add server" }).click()]);
    await expect(setup).toHaveURL(/\/server-setup\?mock$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Connect a host" })).toBeVisible();
    await expect(setup.getByText(/^Step 1 of \d+$/)).toBeVisible();
  });

  test("Cloud lists the accounts", async ({ page }) => {
    await navButton(page, "Cloud").click();
    await expect(pageTitle(page)).toHaveText("Cloud");
    const main = page.locator("main");
    await expect(main.getByText("AWS sandbox", { exact: true })).toBeVisible();
    await expect(main).toContainText("eu-west-3");
    await expect(main.getByText("Microsoft Azure", { exact: true })).toBeVisible();
    await expect(main.getByText("Google Cloud", { exact: true })).toBeVisible();
  });

  test("Set up cloud provider opens the cloud setup", async ({ page, context }) => {
    await navButton(page, "Cloud").click();
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Set up cloud provider" }).click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Set up cloud provider" })).toBeVisible();
  });
});
