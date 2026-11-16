import { test, expect, openApp, answerConfirm } from "./fixtures";

// Cloud labs (EC2 SSRF, cloud services in the player's AWS account) through the dev mock: a cloud
// start streams a Terraform log and leaves outputs and an auto-stop. `cloud=none`: no cloud
// account saved yet (see src/lib/dev-mock/scenario.ts).

test.describe("cloud labs", () => {
  test("with a saved AWS account: the cost first, then outputs and the auto-stop, then stop", async ({ page, app, lab, labs }) => {
    await openApp(page);
    await labs.openLab("EC2 SSRF", { search: "ssrf" });
    await expect(app.main).toContainText("Start the lab to create its cloud services in your account.");

    // Where: only the cloud accounts, the saved one picked, what it costs said before the start.
    const where = await lab.askWhere();
    await expect(where.option(/^This machine/)).toHaveCount(0);
    await expect(where.option(/AWS sandbox/)).toHaveAttribute("aria-checked", "true");
    await expect(where.dialog).toContainText("Billed to your cloud account.");
    await expect(where.dialog).toContainText("about $0.02 an hour (about $0.08 until it auto-stops in 4 h)");
    await expect(where.dialog).toContainText("It lets in your public IP only.");
    await where.agreeToCloudTerraform();
    await where.start();

    // Terraform's steps, then the lab's services in the account.
    const steps = app.main.getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^Set up Terraform/ })).toBeVisible();
    await expect(steps.getByRole("button", { name: /^Create the infrastructure/ })).toBeVisible();
    await expect(app.button("Stop & remove")).toBeEnabled();
    // Where it runs: the cloud and its region in the header, the account on its services' panel.
    await expect(app.main).toContainText("Running·on AWS · eu-west-3");
    await expect(app.main).toContainText(/Auto-stops at \d{2}:\d{2}( [AP]M)? · in [34]h/);
    await expect(app.main.getByRole("heading", { level: 3, name: "Cloud services" })).toBeVisible();
    await expect(app.main.getByText("AWS sandbox", { exact: true })).toBeVisible();
    const services = app.main;
    await expect(services).toContainText("Sign in with access_key_id and secret_key, then find the Lambda function.");
    await expect(services).toContainText("AKIAZ7EXAMPLE4SSRF01");
    // The secret stays masked until asked.
    await expect(services).not.toContainText("wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF");
    await services.getByRole("button", { name: "Show secret_key" }).click();
    await expect(services).toContainText("wJalrXUtnFEMI/K7MDENG/bPxRfiCYSSRF");
    // No attack box, no pause: the player works from their own machine, and cloud labs only stop.
    await expect(app.main.getByRole("heading", { level: 3, name: /^Attack/ })).toHaveCount(0);
    await expect(app.button("Pause")).toHaveCount(0);

    const asked = answerConfirm(page, true);
    await app.button("Stop & remove").click();
    expect(await asked).toContain("Remove EC2 SSRF?");
    expect(await asked).toContain("Its cloud services are destroyed in your account");
    await expect(app.button("Start lab")).toBeEnabled();
    await expect(app.main).toContainText("Start the lab to create its cloud services in your account.");
  });

  // The lab page follows the accounts saved in the setup window (`server:changed`): once one is
  // connected, Start is there, without leaving the page or connecting a second one.
  test("with no account: Connect opens the cloud setup, and once connected the lab can start", async ({ page, app, context, labs, lab }) => {
    await openApp(page, "&cloud=none");
    await labs.openLab("EC2 SSRF", { search: "ssrf" });
    const [setup] = await Promise.all([context.waitForEvent("page"), app.button("Connect an account on AWS").click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock&cloud=none$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Set up cloud provider" })).toBeVisible();
    for (let n = 0; n < 4; n++) await setup.getByRole("button", { name: "Continue", exact: true }).click();
    await setup.getByRole("textbox", { name: /^Name/ }).fill("Lab AWS");
    await setup.getByRole("button", { name: "Save and test", exact: true }).click();
    await expect(setup.getByRole("heading", { level: 1, name: "Lab AWS connected" })).toBeVisible();

    await expect(app.button("Start lab")).toBeEnabled();
    const where = await lab.askWhere();
    await where.expectPicked(where.option(/Lab AWS/));
  });

  test("with no account: the Connect flow saves one, and the lab can start on it after coming back to its page", async ({ page, app, context, lab, labs }) => {
    await openApp(page, "&cloud=none");
    await labs.openLab("EC2 SSRF", { search: "ssrf" });
    await expect(app.button("Start lab")).toHaveCount(0);
    const [setup] = await Promise.all([context.waitForEvent("page"), app.button("Connect an account on AWS").click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock&cloud=none$/);
    await expect(setup.getByText("Step 1 of 6")).toBeVisible();
    for (let n = 0; n < 4; n++) await setup.getByRole("button", { name: "Continue", exact: true }).click();
    await setup.getByRole("textbox", { name: /^Name/ }).fill("Lab AWS");
    await setup.getByRole("button", { name: "Save and test", exact: true }).click();
    await expect(setup.getByRole("heading", { level: 1, name: "Lab AWS connected" })).toBeVisible();
    await setup.close();

    // The Cloud screen lists it; the lab page, opened again, starts on it.
    await app.nav("Cloud").click();
    await expect(app.main.getByText("Lab AWS", { exact: true })).toBeVisible();
    await labs.openLab("EC2 SSRF", { search: "ssrf" });
    const where = await lab.askWhere();
    await expect(where.option(/Lab AWS/)).toHaveAttribute("aria-checked", "true");
    await where.agreeToCloudTerraform();
    await where.start();
    await expect(app.main).toContainText("Running·on AWS · eu-west-3");
    await expect(app.main.getByText("Lab AWS", { exact: true })).toBeVisible();
    const asked = answerConfirm(page, true);
    await app.button("Stop & remove").click();
    expect(await asked).toContain("Remove EC2 SSRF?");
    await expect(app.button("Start lab")).toBeEnabled();
  });
});
