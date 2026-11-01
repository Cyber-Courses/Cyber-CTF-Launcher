import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// The server and cloud setup windows end to end. The dev mock saves what the setup saves (shared
// with the main window, which lists it) and mirrors a few of the core's checks on a save
// (runtime/server/input.rs): a Proxmox user needs a realm, a host is an address without a
// scheme, an AWS access key id starts with AKIA.

const stepOf = (page: Page) => page.getByText(/^Step \d+ of \d+$/);
const button = (page: Page, name: string) => page.getByRole("button", { name, exact: true });
const field = (page: Page, label: string) => page.getByRole("textbox", { name: new RegExp(`^${label}`) });

test.describe("server setup", () => {
  test("Proxmox: Back keeps what was typed, a refused save says why, then it connects and the main window lists it", async ({ page, context }) => {
    await openApp(page);
    await navButton(page, "Servers").click();
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Add server" }).click()]);

    await expect(stepOf(setup)).toHaveText("Step 1 of 5");
    await expect(setup.getByRole("radio", { name: /^Proxmox VE/ })).toHaveAttribute("aria-checked", "true");
    await button(setup, "Continue").click();
    await expect(stepOf(setup)).toHaveText("Step 2 of 5");
    await expect(setup.getByText("Terraform", { exact: true })).toBeVisible();
    await button(setup, "Continue").click();

    // Connection: Continue waits for the host, the user and the password.
    await expect(stepOf(setup)).toHaveText("Step 3 of 5");
    await expect(setup.getByRole("heading", { level: 2, name: "Connect to Proxmox VE" })).toBeVisible();
    const next = button(setup, "Continue");
    await expect(next).toBeDisabled();
    await field(setup, "Name").fill("garage-pve");
    await field(setup, "Host").fill("https://10.0.0.5:8006");
    await expect(next).toBeDisabled();
    await field(setup, "Username").fill("root");
    await expect(next).toBeDisabled();
    await field(setup, "Password").fill("s3cret");
    await expect(next).toBeEnabled();
    await next.click();

    // Placement, then Back: the connection fields are as typed.
    await expect(stepOf(setup)).toHaveText("Step 4 of 5");
    await field(setup, "Node").fill("pve2");
    await button(setup, "Back").click();
    await expect(stepOf(setup)).toHaveText("Step 3 of 5");
    await expect(field(setup, "Name")).toHaveValue("garage-pve");
    await expect(field(setup, "Host")).toHaveValue("https://10.0.0.5:8006");
    await expect(field(setup, "Username")).toHaveValue("root");
    await expect(field(setup, "Password")).toHaveValue("s3cret");
    await button(setup, "Continue").click();
    await expect(field(setup, "Node")).toHaveValue("pve2");

    // The core refuses the address with a scheme, then the user without a realm: said each time,
    // and the step stays.
    await button(setup, "Save and test").click();
    await expect(setup.getByText("host must be a hostname or IP address, without https:// or a path")).toBeVisible();
    await expect(stepOf(setup)).toHaveText("Step 4 of 5");
    await button(setup, "Back").click();
    await field(setup, "Host").fill("10.0.0.5");
    await button(setup, "Continue").click();
    await button(setup, "Save and test").click();
    await expect(setup.getByText("Proxmox users include a realm, e.g. root@pam")).toBeVisible();
    await button(setup, "Back").click();
    await field(setup, "Username").fill("root@pam");
    await button(setup, "Continue").click();
    await button(setup, "Save and test").click();

    // Saved and tested.
    await expect(stepOf(setup)).toHaveText("Step 5 of 5");
    await expect(setup.getByRole("heading", { level: 1, name: "garage-pve connected" })).toBeVisible();
    await expect(setup.getByText(/^Connected/)).toBeVisible();
    await expect(button(setup, "Done")).toBeVisible();

    // The main window heard about it and lists the new host.
    await expect(page.locator("main").getByText("garage-pve", { exact: true })).toBeVisible();
    await expect(page.locator("main")).toContainText("10.0.0.5");
  });

  test("ESXi: the tools step waits for VMware's OVF Tool", async ({ page }) => {
    await page.goto("/server-setup?mock");
    await setup(page)
      .getByRole("radio", { name: /^VMware ESXi/ })
      .click();
    await button(page, "Continue").click();
    await expect(stepOf(page)).toHaveText("Step 2 of 5");
    await expect(page.locator("main")).toContainText("OVF Tool");
    await expect(button(page, "Continue")).toBeDisabled();
    await button(page, "Back").click();
    await expect(page.getByRole("radio", { name: /^VMware ESXi/ })).toHaveAttribute("aria-checked", "true");
  });
});

/** The setup window's page (its own document). */
const setup = (page: Page) => page.locator("main");

test.describe("cloud setup", () => {
  test("AWS with the CLI: Back keeps the region and the name, then it connects", async ({ page, context }) => {
    await openApp(page);
    await navButton(page, "Cloud").click();
    await expect(pageTitle(page)).toHaveText("Cloud");
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Set up cloud provider" }).click()]);

    await expect(stepOf(setup)).toHaveText("Step 1 of 6");
    await expect(setup.getByRole("radio", { name: /^Amazon Web Services/ })).toHaveAttribute("aria-checked", "true");
    await button(setup, "Continue").click();
    await expect(setup.getByText("Amazon Web Services CLI", { exact: true })).toBeVisible();
    await button(setup, "Continue").click();
    await expect(setup.getByRole("radio", { name: /^Use the AWS CLI/ })).toHaveAttribute("aria-checked", "true");
    await button(setup, "Continue").click();

    await expect(stepOf(setup)).toHaveText("Step 4 of 6");
    await expect(setup.getByText("arn:aws:iam::482100000193:user/florian")).toBeVisible();
    await setup.getByRole("combobox", { name: "Region" }).selectOption("eu-central-1");
    await button(setup, "Continue").click();

    await expect(stepOf(setup)).toHaveText("Step 5 of 6");
    await field(setup, "Name").fill("Lab AWS");
    await setup.getByRole("combobox", { name: "Auto-stop" }).selectOption({ label: "2 hours" });
    await button(setup, "Back").click();
    await expect(setup.getByRole("combobox", { name: "Region" })).toHaveValue("eu-central-1");
    await button(setup, "Continue").click();
    await expect(field(setup, "Name")).toHaveValue("Lab AWS");
    await expect(setup.getByRole("combobox", { name: "Auto-stop" })).toHaveValue("2");

    await button(setup, "Save and test").click();
    await expect(stepOf(setup)).toHaveText("Step 6 of 6");
    await expect(setup.getByRole("heading", { level: 1, name: "Lab AWS connected" })).toBeVisible();
    await expect(page.locator("main").getByText("Lab AWS", { exact: true })).toBeVisible();
    await expect(page.locator("main")).toContainText("eu-central-1");
  });

  test("AWS with access keys: both are needed, and a wrong key id is refused with the reason", async ({ page }) => {
    await page.goto("/server-setup?mock&kind=cloud");
    await button(page, "Continue").click();
    await button(page, "Continue").click();
    await page.getByRole("radio", { name: /^Access keys/ }).click();
    await button(page, "Continue").click();

    await expect(stepOf(page)).toHaveText("Step 4 of 6");
    const next = button(page, "Continue");
    await expect(next).toBeDisabled();
    await field(page, "Access key ID").fill("abc123");
    await expect(next).toBeDisabled();
    await page.getByLabel(/^Secret access key/).fill("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
    await expect(next).toBeEnabled();
    await next.click();
    await button(page, "Save and test").click();
    await expect(page.getByText(/access key id looks wrong/)).toBeVisible();

    await button(page, "Back").click();
    await expect(field(page, "Access key ID")).toHaveValue("abc123");
    await field(page, "Access key ID").fill("AKIAIOSFODNN7EXAMPLE");
    await button(page, "Continue").click();
    await button(page, "Save and test").click();
    await expect(page.getByRole("heading", { level: 1, name: /connected$/ })).toBeVisible();
  });

  // BUG: the AWS CLI profile menu lists "default" twice. It always starts with a hard-coded
  // "default" option (value ""), then adds every profile `aws configure list-profiles` prints,
  // which includes "default" whenever the default profile exists (most machines). Picking either
  // works, but the menu reads as broken and an account saved with one shows the other as
  // unselected. Fix (credentials-cli.tsx, AwsCliCredentials): leave "default" out of
  // `s.profiles` when listing them (or drop the hard-coded option when the list has it).
  test("the AWS CLI profile menu lists each profile once", async ({ page }) => {
    await page.goto("/server-setup?mock&kind=cloud");
    for (let n = 0; n < 3; n++) await button(page, "Continue").click();
    const profile = page.getByRole("combobox", { name: "Profile" });
    await expect(profile).toBeVisible();
    const labels = await profile.locator("option").allTextContents();
    expect(labels).toEqual([...new Set(labels)]);
  });
});
