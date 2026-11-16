import { test, expect, openApp } from "./fixtures";
import type { LabScreen } from "./pages/lab";

// A VM lab's and a cloud lab's start failing in the dev mock (`launch=fail`: the first start of
// the page load fails, after a few lines of its log; see src/lib/dev-mock/lab-ops.ts), then Retry
// going through. `deploying=none`: GOAD Light isn't already deploying at load; `os=macos&arch=
// x86_64`: an Intel Mac, where VirtualBox runs it.

/** Start lab (or Retry), then Start in "Where should it run?" on the place already picked; in a
 *  cloud account, once the player agreed to the lab's own Terraform running there (`cloud`). */
async function start(lab: LabScreen, from: "Start lab" | "Retry", picked: RegExp, cloud = false) {
  const where = await lab.askWhere(from);
  await where.expectPicked(where.option(picked));
  if (cloud) await where.agreeToCloudTerraform();
  await where.start();
}

test.describe("a start that fails, then a retry", () => {
  test("a VM lab on this machine: Vagrant's failure under the header, then Retry brings it up", async ({ page, app, lab, labs }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64&launch=fail");
    await labs.openLab("GOAD Light", { search: "goad" });
    await start(lab, "Start lab", /^VM On your system/);

    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice("Couldn’t start GOAD Light")).toContainText("The guest machine entered an invalid state while waiting for it to boot");
    // Said in a toast too; nothing is left to clean up.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" })).toHaveCount(2);
    await expect(app.button("Stop & clean up")).toHaveCount(0);
    await expect(lab.diagram).toHaveCount(0);

    await start(lab, "Retry", /^VM On your system/);
    await expect(app.main).toContainText("Running·on this machine · VirtualBox");
    await expect(app.main).not.toContainText("Failed to start");
    await expect(app.button("Pause")).toBeEnabled();
  });

  test("a cloud lab: Terraform's failure under the header, then Retry creates the services", async ({ page, app, labs, lab }) => {
    await openApp(page, "&launch=fail");
    await labs.openLab("EC2 SSRF", { search: "ssrf" });
    await start(lab, "Start lab", /AWS sandbox/, true);

    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice("Couldn’t start EC2 SSRF")).toContainText(
      "Error: creating EC2 Instance: UnauthorizedOperation: You are not authorized to perform this operation.",
    );
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start EC2 SSRF" })).toHaveCount(2);

    await start(lab, "Retry", /AWS sandbox/, true);
    await expect(app.main).toContainText("Running·on AWS · eu-west-3");
    await expect(app.main).not.toContainText("Failed to start");
    await expect(app.main.getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
    await expect(app.button("Stop & remove")).toBeEnabled();
  });
});
