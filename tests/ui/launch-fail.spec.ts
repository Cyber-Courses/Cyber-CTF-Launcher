import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// A VM lab's and a cloud lab's start failing in the dev mock (`launch=fail`: the first start of
// the page load fails, after a few lines of its log; see src/lib/dev-mock/lab-ops.ts), then Retry
// going through. `deploying=none`: GOAD Light isn't already deploying at load; `os=macos&arch=
// x86_64`: an Intel Mac, where VirtualBox runs it.

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });

async function openLab(page: Page, search: string, title: string) {
  await navButton(page, "Labs").click();
  await page.getByRole("textbox", { name: "Search labs and skills" }).fill(search);
  await main(page)
    .getByRole("button", { name: `Open ${title}`, exact: true })
    .click();
  await expect(pageTitle(page)).toHaveText(title);
}

/** Start lab (or Retry), then Start in "Where should it run?" on the place already picked. */
async function start(page: Page, from: "Start lab" | "Retry", picked: RegExp) {
  await button(page, from).click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  await expect(where.getByRole("radio", { name: picked })).toHaveAttribute("aria-checked", "true");
  await where.getByRole("button", { name: "Start", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("a start that fails, then a retry", () => {
  test("a VM lab on this machine: Vagrant's failure under the header, then Retry brings it up", async ({ page }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64&launch=fail");
    await openLab(page, "goad", "GOAD Light");
    await start(page, "Start lab", /^This machine/);

    await expect(main(page)).toContainText("Failed to start");
    const notice = main(page).getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" });
    await expect(notice).toContainText("The guest machine entered an invalid state while waiting for it to boot");
    // Said in a toast too; nothing is left to clean up.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" })).toHaveCount(2);
    await expect(button(page, "Stop & clean up")).toHaveCount(0);
    await expect(page.locator(".react-flow")).toHaveCount(0);

    await start(page, "Retry", /^This machine/);
    await expect(main(page)).toContainText("Running·on this machine · VirtualBox");
    await expect(main(page)).not.toContainText("Failed to start");
    await expect(button(page, "Pause")).toBeEnabled();
  });

  test("a cloud lab: Terraform's failure under the header, then Retry creates the services", async ({ page }) => {
    await openApp(page, "&launch=fail");
    await openLab(page, "ssrf", "EC2 SSRF");
    await start(page, "Start lab", /AWS sandbox/);

    await expect(main(page)).toContainText("Failed to start");
    const notice = main(page).getByRole("alert").filter({ hasText: "Couldn’t start EC2 SSRF" });
    await expect(notice).toContainText("Error: creating EC2 Instance: UnauthorizedOperation: You are not authorized to perform this operation.");
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start EC2 SSRF" })).toHaveCount(2);

    await start(page, "Retry", /AWS sandbox/);
    await expect(main(page)).toContainText("Running·on AWS sandbox");
    await expect(main(page)).not.toContainText("Failed to start");
    await expect(main(page).getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
    await expect(button(page, "Stop & remove")).toBeEnabled();
  });
});
