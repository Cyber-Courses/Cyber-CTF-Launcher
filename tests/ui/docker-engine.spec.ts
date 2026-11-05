import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// The container engine's states as the system check reports them (`docker=stopped`: installed but
// not running; `docker=denied`: running but refusing this account, with the core's hint and its
// code; see src/lib/dev-mock/scenario.ts), on each screen that reads the report: the Overview,
// the header, the Machine page, a container lab's readiness in the list and in "Where should it
// run?". `deploying=none`: the Overview's second column is this machine's summary, not GOAD
// Light's deploy.

const main = (page: Page) => page.locator("main");
const header = (page: Page) => page.locator("main > [data-tauri-drag-region]");
/** The lab row's title cell, which reads what this machine is missing in place of the description. */
const rowCell = (page: Page, title: string) =>
  main(page)
    .getByRole("button", { name: `Open ${title}`, exact: true })
    .locator("xpath=..");
/** The Machine page's Container labs row. */
const containerRow = (page: Page) => main(page).getByText("Container labs", { exact: true }).locator("xpath=ancestor::div[contains(@class,'border-t')][1]");

async function expectOverviewDown(page: Page) {
  await expect(main(page)).toContainText("Set up Docker to start running labs.");
  await expect(main(page).getByRole("button", { name: "Set up this machine" })).toBeVisible();
  await expect(main(page)).not.toContainText("This machine is ready.");
  // This machine's summary: the engine stopped.
  const engine = main(page).getByText("Docker engine", { exact: true }).locator("xpath=..");
  await expect(engine).toContainText("Stopped");
  await expect(header(page)).toContainText("Docker not running");
}

async function expectLabsNeedEngine(page: Page, readiness: string) {
  await navButton(page, "Labs").click();
  await expect(pageTitle(page)).toHaveText("Labs");
  for (const title of ["Blind orders", "Exposed dashboard"]) {
    await expect(rowCell(page, title)).toContainText(readiness);
    await expect(rowCell(page, title).locator("xpath=..").getByRole("button", { name: "Set up", exact: true })).toBeVisible();
  }
  // A VM lab doesn't need the engine.
  await expect(rowCell(page, "GOAD Light")).not.toContainText(readiness);
  // The lab already running stays so.
  await expect(rowCell(page, "Invoice portal API")).not.toContainText(readiness);
}

async function expectEngineStoppedInDialog(page: Page) {
  await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
  await expect(pageTitle(page)).toHaveText("Blind orders");
  await main(page).getByRole("button", { name: "Start lab", exact: true }).click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  const here = where.getByRole("radio", { name: /This machine On your system/ });
  await expect(here).toContainText("Engine stopped");
  await expect(here).toHaveAttribute("aria-checked", "true");
  await where.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("Docker engine states", () => {
  test("engine stopped: the Overview, the header, the Machine page and the labs say to start it", async ({ page }) => {
    await openApp(page, "&docker=stopped&deploying=none");
    await expectOverviewDown(page);

    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");
    await expect(main(page)).toContainText("1 lab type needs setup");
    await expect(containerRow(page)).toContainText("Engine stopped");
    await expect(containerRow(page)).toContainText("A container engine is installed but not running. Start it to run labs.");
    await expect(containerRow(page).getByRole("button", { name: "Fix", exact: true })).toBeVisible();
    // VM labs are fine: UTM on this Apple Silicon Mac.
    await expect(main(page)).toContainText("UTM");

    await expectLabsNeedEngine(page, "Start your container engine");
    await expectEngineStoppedInDialog(page);
  });

  test("engine denied (Linux): the Overview and the header, then the Machine page's hint with the group, and the labs", async ({ page }) => {
    await openApp(page, "&os=linux&docker=denied&deploying=none");
    await expectOverviewDown(page);

    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");
    await expect(main(page)).toContainText("1 lab type needs setup");
    await expect(containerRow(page)).toContainText("No permission");
    // The core's hint, by its code, in the player's language, with the account to add.
    await expect(containerRow(page)).toContainText(
      "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker florian), then log out and back in.",
    );
    // Nothing to fix from here: the group is joined in a terminal.
    await expect(containerRow(page).getByRole("button", { name: "Fix", exact: true })).toHaveCount(0);

    await expectLabsNeedEngine(page, "Docker needs permission (see Machine)");
    await expectEngineStoppedInDialog(page);
  });

  test("engine denied (Windows): the hint names the docker-users group", async ({ page }) => {
    await openApp(page, "&os=windows&arch=x86_64&docker=denied&deploying=none");
    await expect(header(page)).toContainText("Docker not running");
    await navButton(page, "Machine").click();
    await expect(containerRow(page)).toContainText("No permission");
    await expect(containerRow(page)).toContainText(
      'Docker is running, but your account can\'t use it yet. Add yourself to the docker-users group (in a terminal run as administrator: net localgroup docker-users "florian" /add), then sign out and back in.',
    );
  });
});
