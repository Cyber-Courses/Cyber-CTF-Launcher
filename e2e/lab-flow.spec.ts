import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, breadcrumb, release } from "./fixtures";

// A lab from the catalogue to running and back, through the dev mock's lab operations: a start
// streams a Docker Compose log and leaves the lab running, Stop & remove takes it down
// (`launch=fail` makes the first start fail, `hold=launch` keeps it starting until the test lets
// it end, see src/lib/dev-mock/scenario.ts).

const row = (page: Page, title: string) => page.locator("main").getByRole("button", { name: `Open ${title}`, exact: true });
const deployment = (page: Page) => page.locator("main").getByRole("heading", { level: 3, name: /^Deployment/ });

/** Opens a lab from the list, found through the search. */
async function openLab(page: Page, search: string, title: string) {
  await navButton(page, "Labs").click();
  await page.getByRole("textbox", { name: "Search labs and skills" }).fill(search);
  await expect(page.locator("main").getByRole("button", { name: /^Open / })).toHaveCount(1);
  await row(page, title).click();
  await expect(pageTitle(page)).toHaveText(title);
  await expect(breadcrumb(page)).toHaveText(title);
}

/** Start lab, then Start in "Where should it run?" (this machine, random ports). */
async function startHere(page: Page) {
  await page.getByRole("button", { name: "Start lab" }).click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  await expect(where).toBeVisible();
  await expect(where.getByRole("radio", { name: /This machine/ })).toHaveAttribute("aria-checked", "true");
  await expect(where.getByRole("radio", { name: /Random ports/ })).toHaveAttribute("aria-checked", "true");
  await where.getByRole("button", { name: "Start", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("lab flow", () => {
  test("search, open, start, watch the steps, open the shell, stop", async ({ page, context }) => {
    await openApp(page, "&hold=launch");
    await openLab(page, "dashboard", "Exposed dashboard");
    await expect(page.locator("main")).toContainText("Not started");

    await startHere(page);
    // Starting (held until the test has seen it): the button waits, the deployment lists its
    // steps as the log streams in. The header's button (the sidebar lists the starting lab too).
    await expect(page.locator("main").getByRole("button", { name: /Starting…/ })).toBeDisabled();
    await expect(deployment(page)).toBeVisible();
    const steps = page.locator("main").getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^Download the lab/ })).toBeVisible();
    await expect(steps.getByRole("button", { name: /^Get images/ })).toBeVisible();
    await release(page, "launch");

    // Running: its network, with the lab's machine and the attack box.
    await expect(page.getByRole("button", { name: "Stop & remove" })).toBeEnabled();
    await expect(page.locator("main")).toContainText("Running·on this machine");
    const diagram = page.locator(".react-flow");
    await expect(diagram).toBeVisible();
    await expect(diagram.locator(".react-flow__node").filter({ hasText: "10.43.0.2" })).toContainText("web");
    await expect(diagram.locator(".react-flow__node").filter({ hasText: "Attack box" })).toBeVisible();
    // The sidebar lists it with the running labs.
    await expect(page.locator("aside").getByRole("button", { name: /Exposed dashboard/ })).toBeVisible();

    // The attack box shell opens in its own window (a tab here, answered by the mock's terminal).
    const [shell] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Open attacker shell" }).click()]);
    await expect(shell).toHaveURL(/\/shell\?id=lab-kube&kind=lab&runtime=DOCKER&mock&hold=launch$/);
    await expect(shell.getByText("attack box", { exact: true })).toBeVisible();
    await expect(shell.getByRole("button", { name: "System terminal" })).toBeVisible();
    const terminal = shell.locator(".xterm-rows");
    await expect(terminal).toContainText("root@attack-box:~#");
    await shell.locator(".xterm").click();
    await shell.keyboard.type("id");
    await shell.keyboard.press("Enter");
    await expect(terminal).toContainText("uid=0(root)");
    await shell.close();

    // Stop & remove asks first, then takes it down.
    await page.getByRole("button", { name: "Stop & remove" }).click();
    await page.getByRole("alertdialog", { name: "Remove Exposed dashboard?" }).getByRole("button", { name: "Remove" }).click();
    await expect(page.getByRole("button", { name: "Start lab" })).toBeEnabled();
    await expect(page.locator("main")).toContainText("Not started");
    await expect(page.locator(".react-flow")).toHaveCount(0);
  });

  test("a failed start shows the error, and starting again works", async ({ page }) => {
    await openApp(page, "&launch=fail");
    await openLab(page, "dashboard", "Exposed dashboard");
    await startHere(page);

    await expect(page.locator("main").getByText("Failed", { exact: true })).toBeVisible();
    await expect(page.locator("main")).toContainText("port is already allocated");
    // Not running: the header offers to retry (and on random ports, the port being taken).
    await expect(page.locator("main").getByRole("button", { name: "Retry", exact: true })).toBeEnabled();
    await expect(page.locator("main").getByRole("button", { name: "Retry with random ports" })).toBeEnabled();
    await expect(page.locator(".react-flow")).toHaveCount(0);

    // Retry asks where to run again, as Start does.
    await page.locator("main").getByRole("button", { name: "Retry", exact: true }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Start", exact: true }).click();
    await expect(page.getByRole("button", { name: "Stop & remove" })).toBeEnabled();
    await expect(page.locator("main").getByText("Failed", { exact: true })).toHaveCount(0);
    await expect(page.locator("main")).not.toContainText("port is already allocated");
    await expect(page.locator(".react-flow")).toBeVisible();
  });

  test("a running lab's page survives a round trip through other screens", async ({ page }) => {
    await openApp(page);
    await openLab(page, "invoice", "Invoice portal API");
    await expect(page.locator(".react-flow")).toBeVisible();
    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");
    // The sidebar's running lab opens its page again.
    await page
      .locator("aside")
      .getByRole("button", { name: /Invoice portal API/ })
      .click();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");
    await expect(navButton(page, "Labs")).toHaveAttribute("aria-current", "page");
    await expect(page.locator(".react-flow")).toBeVisible();
  });
});
