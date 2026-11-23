import { test, expect, openApp } from "../fixtures";
import type { AppShell } from "../pages/app-shell";
import type { LabScreen } from "../pages/lab";
import type { LabsScreen } from "../pages/labs";

// The container engine's states as the system check reports them (`docker=stopped`: installed but
// not running; `docker=denied`: running but refusing this account, with the core's hint and its
// code; see src/lib/dev-mock/scenario.ts), on each screen that reads the report: the Overview,
// the header, the Machine page, a container lab's readiness in the list and in "Where should it
// run?". `deploying=none`: the Overview's second column is this machine's summary, not GOAD
// Light's deploy.

/** The Machine page's row of the engine container labs use (the first provider). */
const containerRow = (app: AppShell) => app.main.getByRole("list", { name: "Providers" }).getByRole("listitem").first();

async function expectOverviewDown(app: AppShell) {
  await expect(app.main).toContainText("Set up Docker to start running labs.");
  await expect(app.main.getByRole("button", { name: "Set up this machine" })).toBeVisible();
  await expect(app.main).not.toContainText("This machine is ready.");
  // This machine's summary: the engine stopped.
  const engine = app.main.getByText("Docker engine", { exact: true }).locator("xpath=..");
  await expect(engine).toContainText("Stopped");
  await expect(app.header).toContainText("Docker not running");
}

/** On the Labs screen, each container lab's row reads what this machine is missing (`readiness`)
 *  in place of its description, and offers Set up. */
async function expectLabsNeedEngine(app: AppShell, labs: LabsScreen, readiness: string) {
  await labs.goTo();
  await expect(app.title).toHaveText("Labs");
  for (const title of ["Blind orders", "Exposed dashboard"]) {
    await expect(labs.titleCell(title)).toContainText(readiness);
    await expect(labs.row(title).getByRole("button", { name: "Set up", exact: true })).toBeVisible();
  }
  // A VM lab doesn't need the engine.
  await expect(labs.titleCell("GOAD Light")).not.toContainText(readiness);
  // The lab already running stays so.
  await expect(labs.titleCell("Invoice portal API")).not.toContainText(readiness);
}

async function expectEngineStoppedInDialog(labs: LabsScreen, lab: LabScreen) {
  await labs.openLab("Blind orders");
  const where = await lab.askWhere();
  const here = where.option(/On your system/);
  await expect(here).toContainText("Engine stopped");
  await where.expectPicked(here);
  await where.cancel();
}

test.describe("Docker engine states", () => {
  test("engine stopped: the Overview, the header, the Machine page and the labs say to start it", async ({ page, app, labs, lab }) => {
    await openApp(page, "&docker=stopped&deploying=none");
    await expectOverviewDown(app);

    await app.nav("Machine").click();
    await expect(app.title).toHaveText("Machine");
    await expect(app.main).toContainText("1 lab type needs setup");
    await expect(containerRow(app)).toContainText("Stopped");
    await expect(containerRow(app)).toContainText("A container engine is installed but not running. Start it to run labs.");
    await expect(containerRow(app).getByRole("button", { name: "Fix", exact: true })).toBeVisible();
    // VM labs are fine: UTM on this Apple Silicon Mac.
    await expect(app.main).toContainText("UTM");

    await expectLabsNeedEngine(app, labs, "Start your container engine");
    await expectEngineStoppedInDialog(labs, lab);
  });

  test("engine denied (Linux): the Overview and the header, then the Machine page's hint with the group, and the labs", async ({ page, app, labs, lab }) => {
    await openApp(page, "&os=linux&docker=denied&deploying=none");
    await expectOverviewDown(app);

    await app.nav("Machine").click();
    await expect(app.title).toHaveText("Machine");
    await expect(app.main).toContainText("1 lab type needs setup");
    await expect(containerRow(app)).toContainText("No permission");
    // The core's hint, by its code, in the player's language, with the account to add.
    await expect(containerRow(app)).toContainText(
      "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker ada), then log out and back in.",
    );
    // Nothing to fix from here: the group is joined in a terminal.
    await expect(containerRow(app).getByRole("button", { name: "Fix", exact: true })).toHaveCount(0);

    await expectLabsNeedEngine(app, labs, "Docker needs permission (see Machine)");
    await expectEngineStoppedInDialog(labs, lab);
  });

  test("engine denied (Windows): the hint names the docker-users group", async ({ page, app }) => {
    await openApp(page, "&os=windows&arch=x86_64&docker=denied&deploying=none");
    await expect(app.header).toContainText("Docker not running");
    await app.nav("Machine").click();
    await expect(containerRow(app)).toContainText("No permission");
    await expect(containerRow(app)).toContainText(
      'Docker is running, but your account can\'t use it yet. Add yourself to the docker-users group (in a terminal run as administrator: net localgroup docker-users "ada" /add), then sign out and back in.',
    );
  });
});
