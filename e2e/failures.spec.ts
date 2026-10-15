import { test, expect, openApp, navButton, pageTitle, forceCommands, releaseForced } from "./fixtures";

// What the app shows when something fails: the catalogue API, a lab's start or removal, the
// machine check. The dev mock answers everything else (Invoice portal API runs, GOAD Light
// deploys on a server).

const PORT_CLASH = "Host port 8080 is already in use on this machine (another lab or app holds it). Stop whatever uses it, or start the lab with random ports.";

const main = (page: import("@playwright/test").Page) => page.locator("main");
const sidebar = (page: import("@playwright/test").Page) => page.locator("aside");

test.describe("failure states", () => {
  test("labs running here stay listed while the catalogue can't be reached", async ({ page }) => {
    await forceCommands(page, { api_query: { error: "error sending request for url (https://api.cyberctf.org/graphql): connection refused" } });
    await openApp(page);
    // The Overview: the running lab, by its id, and the catalogue's error with a retry.
    await expect(main(page)).toContainText("1 lab running on this machine.");
    await expect(main(page).getByText("Invoice", { exact: true })).toBeVisible();
    await expect(main(page)).toContainText("Can’t reach the lab catalogue");
    await expect(main(page).getByRole("button", { name: "Retry" })).toBeVisible();
    await expect(main(page)).not.toContainText("Loading labs…");
    // The sidebar keeps its Running section: the running lab and the one deploying.
    await expect(sidebar(page)).toContainText("Invoice");
    await expect(sidebar(page)).toContainText("Goad");
    // The Labs screen lists it above the error too.
    await navButton(page, "Labs").click();
    await expect(main(page)).toContainText("Running now");
    await expect(main(page)).toContainText("lab-invoice");
  });

  test("a start that fails says so in the header, with Retry and the port hint", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: PORT_CLASH, delay: 300 } });
    await page.addInitScript(() => localStorage.setItem("cyberctf.labs.ports", "default"));
    await openApp(page);
    await navButton(page, "Labs").click();
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Blind orders");
    await page.getByRole("button", { name: "Start lab" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Start", exact: true }).click();

    await expect(main(page)).toContainText("Failed to start");
    await expect(main(page)).not.toContainText("Not started");
    await expect(page.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Retry with random ports" })).toBeVisible();
    await expect(main(page).getByRole("alert").filter({ hasText: "Couldn’t start Blind orders" })).toContainText("Start it on random ports instead.");

    // The list says it too.
    await navButton(page, "Labs").click();
    // Its row: the title's cell reads "Failed to start" in place of the description.
    await expect(main(page).getByRole("button", { name: "Open Blind orders", exact: true }).locator("xpath=..")).toContainText("Failed to start");
  });

  test("a failed Stop & remove keeps the panels and says so under the header", async ({ page }) => {
    await forceCommands(page, { lab_stop: { error: "Couldn't remove network lab-invoice_default: has active endpoints", held: true } });
    await openApp(page);
    await navButton(page, "Labs").click();
    await main(page).getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    const diagram = main(page).getByText("Your machine", { exact: true });
    await expect(diagram).toBeVisible();
    await page.getByRole("button", { name: "Stop & remove" }).click();
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: /Remove/ })
      .click();

    // While it stops (held until the test has seen it): the diagram stays (dimmed), the header
    // says Stopping.
    await expect(main(page)).toContainText("Stopping");
    await expect(diagram).toBeVisible();
    await releaseForced(page, "lab_stop");
    // Then the failure, under the header, in view.
    const notice = main(page).getByRole("alert").filter({ hasText: "Couldn’t stop and remove Invoice portal API" });
    await expect(notice).toBeVisible();
    // And a toast, wherever the player is: the notice and the toast.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t stop and remove Invoice portal API" })).toHaveCount(2);
    await expect(notice).toBeInViewport();
    await expect(notice).toContainText("Part of the lab is still up.");
    await expect(diagram).toBeVisible();
  });

  test("a deploy with no log here shows the backend's step", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await main(page).getByRole("button", { name: "Open GOAD Light", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("GOAD Light");
    await expect(main(page)).toContainText("Configure SRV02 (ansible 41/88)");
    await expect(main(page)).not.toContainText("Preparing…");
    await expect(main(page)).not.toContainText("Building");
  });

  test("a failed machine check offers Try again, on the Overview and in the setup window", async ({ page }) => {
    await forceCommands(page, { system_check: { error: "failed to run `docker info`: permission denied" } });
    await openApp(page);
    await expect(main(page)).toContainText("Couldn’t check this machine");
    await expect(main(page)).toContainText("permission denied");
    await expect(main(page).getByRole("button", { name: "Try again" })).toBeVisible();
    await expect(main(page)).not.toContainText("Checking this machine…");

    await page.goto("/machine-setup?mock");
    await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
    await expect(page.getByText("permission denied")).toBeVisible();
  });
});
