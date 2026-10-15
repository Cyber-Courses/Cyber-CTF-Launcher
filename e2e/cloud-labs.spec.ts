import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, release } from "./fixtures";

// Cloud labs (EC2 SSRF, cloud services in the player's AWS account) and hosted labs (Blind orders,
// run by Cyber CTF) through the dev mock: a cloud start streams a Terraform log and leaves outputs
// and an auto-stop; a hosted session is requested, then runs on the next poll (`hold=hosted`: the
// first poll after the test lets it). `cloud=none`: no cloud account saved yet (see
// src/lib/dev-mock/scenario.ts).

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
    await expect(main(page)).toContainText("Running·on AWS sandbox");
    await expect(main(page)).toContainText(/Auto-stops at \d{2}:\d{2}( [AP]M)? · in [34]h/);
    await expect(main(page).getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
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

    await button(page, "Stop & remove").click();
    const remove = page.getByRole("alertdialog", { name: "Remove EC2 SSRF?" });
    await expect(remove).toContainText("Its cloud services are destroyed in your account");
    await remove.getByRole("button", { name: "Remove" }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(main(page)).toContainText("Start the lab to create its cloud services in your account.");
  });

  // BUG: connecting a cloud account from a cloud lab's page leaves the page on "Connect an account
  // on AWS". The setup window saves the account and emits `server:changed` (server-setup/page.tsx),
  // but the lab page reads the accounts once (use-run-target.ts, useRunTarget: `serverList()` in an
  // effect on [rt, localReady]) and doesn't listen for the event, so the player clicks Connect
  // again (and connects a second account) or has to leave the page and come back. Fix
  // (use-run-target.ts): read `serverList()` again on `useTauriEvent(SERVER_CHANGED, …)`, as
  // use-server-list.ts does.
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
    await expect(main(page)).toContainText("Running·on Lab AWS");
    await button(page, "Stop & remove").click();
    await page.getByRole("alertdialog", { name: "Remove EC2 SSRF?" }).getByRole("button", { name: "Remove" }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
  });
});

test.describe("hosted labs", () => {
  test("hosted by Cyber CTF: requested, running with its address, then stopped", async ({ page }) => {
    await openApp(page, "&hold=hosted");
    await openLab(page, "blind", "Blind orders");
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await where.getByRole("radio", { name: /Hosted by Cyber CTF/ }).click();
    await where.getByRole("button", { name: "Start", exact: true }).click();

    // Requested, whatever the polls answer until the test has seen it.
    await expect(main(page)).toContainText("Asking Cyber CTF for an instance… This usually takes about 30 seconds.");
    await release(page, "hosted");
    await expect(main(page)).toContainText("Running, hosted by Cyber CTF");
    await expect(main(page)).toContainText(/Expires in (1h 59m|2h 0m)\./);
    await expect(button(page, "Open")).toBeVisible();
    // One run at a time: Start waits for the hosted one to end.
    await expect(button(page, "Start lab")).toBeDisabled();

    await button(page, "Stop").click();
    await expect(main(page)).not.toContainText("Running, hosted by Cyber CTF");
    await expect(button(page, "Start lab")).toBeEnabled();
  });

  // BUG: while the lab runs hosted by Cyber CTF, its page's status line still says "Not started"
  // (and the side rail "Where it runs", the attack box "starts with the lab"), right above the
  // "Running, hosted by Cyber CTF" panel. The status line comes from deriveLabState (lab-state.ts),
  // which only knows the local/server/cloud status, not the hosted session (use-hosted-run.ts).
  // Fix (lab-detail/index.tsx + header-lead.tsx): pass `hosted.live` to HeaderLead and say
  // "Running · hosted by Cyber CTF" when it is, as the Overview's running list would.
  test("a lab running hosted doesn't read as not started", async ({ page }) => {
    await openApp(page);
    await openLab(page, "blind", "Blind orders");
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await where.getByRole("radio", { name: /Hosted by Cyber CTF/ }).click();
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(main(page)).toContainText("Running, hosted by Cyber CTF");
    await expect(main(page)).not.toContainText("Not started");
  });
});
