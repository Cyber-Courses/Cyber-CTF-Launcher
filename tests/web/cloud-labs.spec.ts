import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, answerConfirm } from "./fixtures";

// Cloud labs (EC2 SSRF, cloud services in the player's AWS account) through the dev mock: a cloud
// start streams a Terraform log and leaves outputs and an auto-stop. `cloud=none`: no cloud
// account saved yet (see src/lib/dev-mock/scenario.ts).

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

test.describe("cloud labs", () => {
  test("with a saved AWS account: the cost first, then outputs and the auto-stop, then stop", async ({ page }) => {
    await openApp(page);
    await openLab(page, "ssrf", "EC2 SSRF");
    await expect(main(page)).toContainText("Start the lab to create its cloud services in your account.");

    // Where: only the cloud accounts, the saved one picked, what it costs said before the start.
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: /^This machine/ })).toHaveCount(0);
    await expect(where.getByRole("radio", { name: /AWS sandbox/ })).toHaveAttribute("aria-checked", "true");
    await expect(where).toContainText("Billed to your cloud account.");
    await expect(where).toContainText("about $0.02 an hour (about $0.08 until it auto-stops in 4 h)");
    await expect(where).toContainText("It lets in your public IP only.");
    await where.getByRole("button", { name: "Start", exact: true }).click();

    // Terraform's steps, then the lab's services in the account.
    const steps = main(page).getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^Set up Terraform/ })).toBeVisible();
    await expect(steps.getByRole("button", { name: /^Create the infrastructure/ })).toBeVisible();
    await expect(button(page, "Stop & remove")).toBeEnabled();
    // Where it runs: the cloud and its region in the header, the account on its services' panel.
    await expect(main(page)).toContainText("Running·on AWS · eu-west-3");
    await expect(main(page)).toContainText(/Auto-stops at \d{2}:\d{2}( [AP]M)? · in [34]h/);
    await expect(main(page).getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
    await expect(main(page).getByText("AWS sandbox", { exact: true })).toBeVisible();
    const services = main(page);
    await expect(services).toContainText("Sign in with access_key_id and secret_key, then find the Lambda function.");
    await expect(services).toContainText("AKIAZ7EXAMPLE4SSRF01");
    // The secret stays masked until asked.
    await expect(services).not.toContainText("wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF");
    await services.getByRole("button", { name: "Show secret_key" }).click();
    await expect(services).toContainText("wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF");
    // No attack box, no pause: the player works from their own machine, and cloud labs only stop.
    await expect(main(page).getByRole("heading", { level: 3, name: /^Attack/ })).toHaveCount(0);
    await expect(button(page, "Pause")).toHaveCount(0);

    const asked = answerConfirm(page, true);
    await button(page, "Stop & remove").click();
    expect(await asked).toContain("Remove EC2 SSRF?");
    expect(await asked).toContain("Its cloud services are destroyed in your account");
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(main(page)).toContainText("Start the lab to create its cloud services in your account.");
  });

  // The lab page follows the accounts saved in the setup window (`server:changed`): once one is
  // connected, Start is there, without leaving the page or connecting a second one.
  test("with no account: Connect opens the cloud setup, and once connected the lab can start", async ({ page, context }) => {
    await openApp(page, "&cloud=none");
    await openLab(page, "ssrf", "EC2 SSRF");
    const [setup] = await Promise.all([context.waitForEvent("page"), button(page, "Connect an account on AWS").click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock&cloud=none$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Set up cloud provider" })).toBeVisible();
    for (let n = 0; n < 4; n++) await setup.getByRole("button", { name: "Continue", exact: true }).click();
    await setup.getByRole("textbox", { name: /^Name/ }).fill("Lab AWS");
    await setup.getByRole("button", { name: "Save and test", exact: true }).click();
    await expect(setup.getByRole("heading", { level: 1, name: "Lab AWS connected" })).toBeVisible();

    await expect(button(page, "Start lab")).toBeEnabled();
    await button(page, "Start lab").click();
    await expect(page.getByRole("dialog", { name: "Where should the lab run?" }).getByRole("radio", { name: /Lab AWS/ })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  test("with no account: the Connect flow saves one, and the lab can start on it after coming back to its page", async ({ page, context }) => {
    await openApp(page, "&cloud=none");
    await openLab(page, "ssrf", "EC2 SSRF");
    await expect(button(page, "Start lab")).toHaveCount(0);
    const [setup] = await Promise.all([context.waitForEvent("page"), button(page, "Connect an account on AWS").click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock&cloud=none$/);
    await expect(setup.getByText("Step 1 of 6")).toBeVisible();
    for (let n = 0; n < 4; n++) await setup.getByRole("button", { name: "Continue", exact: true }).click();
    await setup.getByRole("textbox", { name: /^Name/ }).fill("Lab AWS");
    await setup.getByRole("button", { name: "Save and test", exact: true }).click();
    await expect(setup.getByRole("heading", { level: 1, name: "Lab AWS connected" })).toBeVisible();
    await setup.close();

    // The Cloud screen lists it; the lab page, opened again, starts on it.
    await navButton(page, "Cloud").click();
    await expect(main(page).getByText("Lab AWS", { exact: true })).toBeVisible();
    await openLab(page, "ssrf", "EC2 SSRF");
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: /Lab AWS/ })).toHaveAttribute("aria-checked", "true");
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(main(page)).toContainText("Running·on AWS · eu-west-3");
    await expect(main(page).getByText("Lab AWS", { exact: true })).toBeVisible();
    const asked = answerConfirm(page, true);
    await button(page, "Stop & remove").click();
    expect(await asked).toContain("Remove EC2 SSRF?");
    await expect(button(page, "Start lab")).toBeEnabled();
  });
});
