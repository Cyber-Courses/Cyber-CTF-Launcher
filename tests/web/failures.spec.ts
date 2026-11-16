import { test, expect, openApp, forceCommands, releaseForced, answerConfirm } from "./fixtures";

// What the app shows when something fails: the catalogue API, a lab's start or removal, the
// machine check. The dev mock answers everything else (Invoice portal API runs, GOAD Light
// deploys on a server).

// Docker's own words when a port is taken under it (no code: the hint knows them by their words).
const PORT_CLASH =
  "Error response from daemon: driver failed programming external connectivity on endpoint web: Bind for 0.0.0.0:8080 failed: port is already allocated";

test.describe("failure states", () => {
  test("labs running here stay listed while the catalogue can't be reached", async ({ page, app }) => {
    await forceCommands(page, { api_query: { error: "error sending request for url (https://api.cyberctf.com/graphql): connection refused" } });
    await openApp(page);
    // The Overview: the running lab, by its id, and the catalogue's error with a retry.
    await expect(app.main).toContainText("1 lab running on this machine.");
    await expect(app.main.getByText("Invoice", { exact: true })).toBeVisible();
    await expect(app.main).toContainText("Can’t reach the lab catalogue");
    await expect(app.main.getByRole("button", { name: "Retry" })).toBeVisible();
    await expect(app.main).not.toContainText("Loading labs…");
    // The sidebar keeps its Running section: the running lab and the one deploying.
    await expect(app.sidebar).toContainText("Invoice");
    await expect(app.sidebar).toContainText("Goad");
    // The Labs screen lists it above the error too.
    await app.nav("Labs").click();
    await expect(app.main).toContainText("Running now");
    await expect(app.main).toContainText("lab-invoice");
  });

  test("a start that fails says so in the header, with Retry and the port hint", async ({ page, app }) => {
    await forceCommands(page, { lab_launch: { error: PORT_CLASH } });
    await page.addInitScript(() => localStorage.setItem("cyberctf.labs.ports", "default"));
    await openApp(page);
    await app.nav("Labs").click();
    await app.main.getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await expect(app.title).toHaveText("Blind orders");
    await page.getByRole("button", { name: "Start lab" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Start", exact: true }).click();

    await expect(app.main).toContainText("Failed to start");
    await expect(app.main).not.toContainText("Not started");
    await expect(page.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Retry with random ports" })).toBeVisible();
    await expect(app.main.getByRole("alert").filter({ hasText: "Couldn’t start Blind orders" })).toContainText(
      "Stop whatever uses it, or start it with random ports if the lab doesn't need that exact port.",
    );

    // The list says it too.
    await app.nav("Labs").click();
    // Its row: the title's cell reads "Failed to start" in place of the description.
    await expect(app.main.getByRole("button", { name: "Open Blind orders", exact: true }).locator("xpath=..")).toContainText("Failed to start");
  });

  test("a failed Stop & remove keeps the panels and says so under the header", async ({ page, app }) => {
    await forceCommands(page, { lab_stop: { error: "Couldn't remove network lab-invoice_default: has active endpoints", held: true } });
    await openApp(page);
    await app.nav("Labs").click();
    await app.main.getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    const diagram = app.main.locator(".react-flow");
    await expect(diagram).toBeVisible();
    const asked = answerConfirm(page, true);
    await page.getByRole("button", { name: "Stop & remove" }).click();
    await asked;

    // While it stops (held until the test has seen it): the diagram stays (dimmed), the header
    // says Stopping.
    await expect(app.main).toContainText("Stopping");
    await expect(diagram).toBeVisible();
    await releaseForced(page, "lab_stop");
    // Then the failure, under the header, in view.
    const notice = app.main.getByRole("alert").filter({ hasText: "Couldn’t stop and remove Invoice portal API" });
    await expect(notice).toBeVisible();
    // And a toast, wherever the player is: the notice and the toast.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t stop and remove Invoice portal API" })).toHaveCount(2);
    await expect(notice).toBeInViewport();
    await expect(notice).toContainText("Part of the lab is still up.");
    await expect(diagram).toBeVisible();
  });

  test("a deploy with no log here shows the backend's step", async ({ page, app }) => {
    await openApp(page);
    await app.nav("Labs").click();
    await app.main.getByRole("button", { name: "Open GOAD Light", exact: true }).click();
    await expect(app.title).toHaveText("GOAD Light");
    await expect(app.main).toContainText("Configure SRV02 (ansible 41/88)");
    await expect(app.main).not.toContainText("Preparing…");
    await expect(app.main).not.toContainText("Building");
  });

  test("a failed machine check offers Try again, on the Overview and in the setup window", async ({ page, app }) => {
    await forceCommands(page, { system_check: { error: "failed to run `docker info`: permission denied" } });
    await openApp(page);
    await expect(app.main).toContainText("Couldn’t check this machine");
    await expect(app.main).toContainText("permission denied");
    await expect(app.main.getByRole("button", { name: "Try again" })).toBeVisible();
    await expect(app.main).not.toContainText("Checking this machine…");

    await page.goto("/machine-setup?mock");
    await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
    await expect(page.getByText("permission denied")).toBeVisible();
  });
});
