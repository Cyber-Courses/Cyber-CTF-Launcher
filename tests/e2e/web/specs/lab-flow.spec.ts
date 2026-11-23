import { test, expect, openApp, release, answerConfirm } from "../fixtures";
import type { LabsScreen } from "../pages/labs";
import type { LabScreen } from "../pages/lab";

// A lab from the catalogue to running and back, through the dev mock's lab operations: a start
// streams a Docker Compose log and leaves the lab running, Stop & remove takes it down
// (`launch=fail` makes the first start fail, `hold=launch` keeps it starting until the test lets
// it end, see src/lib/dev-mock/scenario.ts).

/** Opens a lab from the list, found through the search, which leaves its row alone. */
async function openLab(labs: LabsScreen, search: string, title: string) {
  await labs.goTo();
  await labs.search.fill(search);
  await expect(labs.openButtons).toHaveCount(1);
  await labs.openLab(title);
}

/** Start lab, then in "Where should it run?": containers on this machine (picked already), then
 *  their host ports (random ones, picked already), and Start. */
async function startHere(lab: LabScreen) {
  const where = await lab.askWhere();
  await where.expectPicked(where.containersHere);
  await where.containersHere.click();
  await where.expectPicked(where.option(/^Random ports/));
  await where.start();
}

test.describe("lab flow", () => {
  test("search, open, start, watch the steps, open the shell, stop", async ({ page, app, labs, lab, context }) => {
    await openApp(page, "&hold=launch");
    await openLab(labs, "dashboard", "Exposed dashboard");
    await expect(app.main).toContainText("Not started");

    await startHere(lab);
    // Starting (held until the test has seen it): the button waits, the deployment lists its
    // steps as the log streams in. The header's button (the sidebar lists the starting lab too).
    await expect(app.main.getByRole("button", { name: /Starting…/ })).toBeDisabled();
    await expect(lab.deployment).toBeVisible();
    await expect(lab.steps.getByRole("button", { name: /^Download the lab/ })).toBeVisible();
    await expect(lab.steps.getByRole("button", { name: /^Get images/ })).toBeVisible();
    await release(page, "launch");

    // Running: its network, with the lab's machine and the attack box.
    await expect(page.getByRole("button", { name: "Stop & remove" })).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine");
    await expect(lab.diagram).toBeVisible();
    await expect(lab.diagramNode("10.43.0.2")).toContainText("web");
    await expect(lab.diagramNode("Attack box")).toBeVisible();
    // Its service is published on a random port of this machine, not its own.
    await expect(app.main.getByRole("button", { name: /^:\d+$/ })).toBeVisible();
    await expect(app.main.getByRole("button", { name: ":8443", exact: true })).toHaveCount(0);
    // The sidebar lists it with the running labs.
    await expect(app.sidebar.getByRole("button", { name: /Exposed dashboard/ })).toBeVisible();

    // The attack box shell opens in its own window (a tab here, answered by the mock's terminal).
    const [shell] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Open attacker shell" }).click()]);
    await expect(shell).toHaveURL(/\/shell\?id=lab-kube&runtime=DOCKER&mock&hold=launch$/);
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
    const asked = answerConfirm(page, true);
    await page.getByRole("button", { name: "Stop & remove" }).click();
    expect(await asked).toContain("Remove Exposed dashboard?");
    await expect(page.getByRole("button", { name: "Start lab" })).toBeEnabled();
    await expect(app.main).toContainText("Not started");
    await expect(lab.diagram).toHaveCount(0);
  });

  test("a failed start shows the error, and starting again works", async ({ page, app, labs, lab }) => {
    await openApp(page, "&launch=fail");
    await openLab(labs, "dashboard", "Exposed dashboard");
    await startHere(lab);

    await expect(app.main.getByText("Failed", { exact: true })).toBeVisible();
    await expect(app.main).toContainText("port is already allocated");
    // Not running: the header offers to retry (and on random ports, the port being taken).
    await expect(app.button("Retry")).toBeEnabled();
    await expect(app.main.getByRole("button", { name: "Retry with random ports" })).toBeEnabled();
    await expect(lab.diagram).toHaveCount(0);

    // Retry asks where to run again, as Start does.
    await (await lab.askWhere("Retry")).start();
    await expect(page.getByRole("button", { name: "Stop & remove" })).toBeEnabled();
    await expect(app.main.getByText("Failed", { exact: true })).toHaveCount(0);
    await expect(app.main).not.toContainText("port is already allocated");
    await expect(lab.diagram).toBeVisible();
  });

  test("a running lab's page survives a round trip through other screens", async ({ page, app, labs, lab }) => {
    await openApp(page);
    await openLab(labs, "invoice", "Invoice portal API");
    await expect(lab.diagram).toBeVisible();
    await app.nav("Machine").click();
    await expect(app.title).toHaveText("Machine");
    // The sidebar's running lab opens its page again.
    await app.sidebar.getByRole("button", { name: /Invoice portal API/ }).click();
    await expect(app.title).toHaveText("Invoice portal API");
    await expect(app.nav("Labs")).toHaveAttribute("aria-current", "page");
    await expect(lab.diagram).toBeVisible();
  });
});
