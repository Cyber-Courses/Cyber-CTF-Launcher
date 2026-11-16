import type { Page } from "@playwright/test";
import { test, expect, agreeToCloudTerraform as agree, openApp } from "./fixtures";
import { AppShell } from "./pages/app-shell";

// A VM lab's and a cloud lab's start failing in the dev mock (`launch=fail`: the first start of
// the page load fails, after a few lines of its log; see src/lib/dev-mock/lab-ops.ts), then Retry
// going through. `deploying=none`: GOAD Light isn't already deploying at load; `os=macos&arch=
// x86_64`: an Intel Mac, where VirtualBox runs it.

async function openLab(page: Page, search: string, title: string) {
  const app = new AppShell(page);
  await app.nav("Labs").click();
  await page.getByRole("textbox", { name: "Search labs and skills" }).fill(search);
  await app.main.getByRole("button", { name: `Open ${title}`, exact: true }).click();
  await expect(app.title).toHaveText(title);
}

/** Start lab (or Retry), then Start in "Where should it run?" on the place already picked; in a
 *  cloud account, once the player agreed to the lab's own Terraform running there (`cloud`). */
async function start(page: Page, from: "Start lab" | "Retry", picked: RegExp, cloud = false) {
  const app = new AppShell(page);
  await app.button(from).click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  await expect(where.getByRole("radio", { name: picked })).toHaveAttribute("aria-checked", "true");
  if (cloud) await agree(where);
  await where.getByRole("button", { name: "Start", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("a start that fails, then a retry", () => {
  test("a VM lab on this machine: Vagrant's failure under the header, then Retry brings it up", async ({ page, app }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64&launch=fail");
    await openLab(page, "goad", "GOAD Light");
    await start(page, "Start lab", /^VM On your system/);

    await expect(app.main).toContainText("Failed to start");
    const notice = app.main.getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" });
    await expect(notice).toContainText("The guest machine entered an invalid state while waiting for it to boot");
    // Said in a toast too; nothing is left to clean up.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" })).toHaveCount(2);
    await expect(app.button("Stop & clean up")).toHaveCount(0);
    await expect(page.locator(".react-flow")).toHaveCount(0);

    await start(page, "Retry", /^VM On your system/);
    await expect(app.main).toContainText("Running·on this machine · VirtualBox");
    await expect(app.main).not.toContainText("Failed to start");
    await expect(app.button("Pause")).toBeEnabled();
  });

  test("a cloud lab: Terraform's failure under the header, then Retry creates the services", async ({ page, app }) => {
    await openApp(page, "&launch=fail");
    await openLab(page, "ssrf", "EC2 SSRF");
    await start(page, "Start lab", /AWS sandbox/, true);

    await expect(app.main).toContainText("Failed to start");
    const notice = app.main.getByRole("alert").filter({ hasText: "Couldn’t start EC2 SSRF" });
    await expect(notice).toContainText("Error: creating EC2 Instance: UnauthorizedOperation: You are not authorized to perform this operation.");
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start EC2 SSRF" })).toHaveCount(2);

    await start(page, "Retry", /AWS sandbox/, true);
    await expect(app.main).toContainText("Running·on AWS · eu-west-3");
    await expect(app.main).not.toContainText("Failed to start");
    await expect(app.main.getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
    await expect(app.button("Stop & remove")).toBeEnabled();
  });
});
