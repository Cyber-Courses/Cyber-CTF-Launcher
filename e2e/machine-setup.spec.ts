import type { BrowserContext, Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, emitEvent } from "./fixtures";

// The guided machine-setup window (`/machine-setup`), opened from the Machine page as the app opens
// it (the dev mock's `machine_open_setup` opens a tab, with `?step=` for a "Fix" link). The mock's
// one-click installs stream an installer's output and succeed, and the next system check reports
// the tool installed; `pm=later` has the package manager show up after the first check (as if the
// player ran Homebrew's install command), `install=fail` fails the first install (see
// src/lib/dev-mock.ts).

const stepOf = (page: Page) => page.getByText(/^Step \d+ of \d+$/);
const stepTitle = (page: Page) => page.getByRole("heading", { level: 3 });
const button = (page: Page, name: string) => page.getByRole("button", { name, exact: true });

/** The setup window the click opens. */
async function opens(context: BrowserContext, click: () => Promise<void>) {
  const [setup] = await Promise.all([context.waitForEvent("page"), click()]);
  await expect(setup.getByRole("heading", { level: 1, name: "Set up this machine" })).toBeVisible();
  return setup;
}

test.describe("machine setup window", () => {
  test("a ready machine: every step from the Machine page's Setup view, Back, and Done closes the window", async ({ page, context }) => {
    await openApp(page);
    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");
    await page.getByRole("radiogroup", { name: "Machine view" }).getByRole("radio", { name: "Setup" }).click();
    const setup = await opens(context, () => page.getByRole("button", { name: "Open guided setup" }).click());
    await expect(setup).toHaveURL(/\/machine-setup\?mock$/);

    await expect(stepOf(setup)).toHaveText("Step 1 of 8");
    await expect(stepTitle(setup)).toHaveText("Package manager");
    await expect(setup.getByText("brew", { exact: true })).toBeVisible();
    // Nothing to go back to on the first step.
    await expect(button(setup, "Back")).toHaveCount(0);
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Container engine");
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toHaveAttribute("aria-checked", "true");
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toContainText("in use");
    await button(setup, "Continue").click();

    // The container test runs on its own; the flow waits for it.
    await expect(stepTitle(setup)).toHaveText("Test container labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Attack machine");
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Virtual machines");
    await expect(stepOf(setup)).toHaveText("Step 5 of 8");
    await expect(setup.getByRole("radio", { name: /^UTM/ })).toHaveAttribute("aria-checked", "true");
    // Back and forth keeps the place.
    await button(setup, "Back").click();
    await expect(stepTitle(setup)).toHaveText("Attack machine");
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("Virtual machines");
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Provisioning");
    await expect(setup.getByText("2.4.3")).toBeVisible();
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Test VM labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    await button(setup, "Continue").click();

    await expect(stepOf(setup)).toHaveText("Step 8 of 8");
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    await expect(setup.getByText("Tested and ready to run.")).toHaveCount(2);
    const closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;
  });

  test("a fresh Mac: the package manager is detected, then each install streams and the next check has it", async ({ page, context }) => {
    test.slow(); // The setup window checks the machine every 5 s; the package manager shows on the second.
    await openApp(page, "&machine=fresh&pm=later&os=macos&arch=arm64");
    await navButton(page, "Machine").click();
    const setup = await opens(context, () => page.getByRole("button", { name: "Set up this machine" }).click());

    // Setup tools: Homebrew's command to run, and Continue waits until it's there.
    await expect(stepTitle(setup)).toHaveText("Package manager");
    await expect(setup.getByText(/raw\.githubusercontent\.com\/Homebrew\/install/)).toBeVisible();
    await expect(button(setup, "Continue")).toBeDisabled();
    await expect(button(setup, "Continue")).toBeEnabled({ timeout: 12_000 });
    await expect(setup.getByText("brew", { exact: true })).toBeVisible();
    await button(setup, "Continue").click();

    // Container engine: install Docker Desktop, its output streams, then it is in use.
    await expect(stepTitle(setup)).toHaveText("Container engine");
    await expect(button(setup, "Continue")).toBeDisabled();
    await button(setup, "Install Docker Desktop").click();
    await expect(setup.getByText("==> Installing docker")).toBeVisible();
    await expect(setup.getByText("docker was successfully installed!", { exact: false })).toBeVisible();
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toContainText("in use");
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Test container labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("Attack machine");
    await button(setup, "Continue").click();

    // Virtual machines: optional (Skip for now) until one is installed; UTM is the one Cyber CTF
    // installs on Apple Silicon.
    await expect(stepTitle(setup)).toHaveText("Virtual machines");
    await expect(button(setup, "Skip for now")).toBeEnabled();
    await expect(setup.getByRole("radio", { name: /^UTM/ })).toHaveAttribute("aria-checked", "true");
    await button(setup, "Install UTM").click();
    await expect(setup.getByText("utm was successfully installed!", { exact: false })).toBeVisible();
    await expect(setup.getByRole("radio", { name: /^UTM/ })).toContainText("installed");
    await button(setup, "Continue").click();

    // Provisioning: Vagrant, then the plugin (which needs Vagrant first).
    await expect(stepTitle(setup)).toHaveText("Provisioning");
    await expect(button(setup, "Continue")).toBeDisabled();
    await expect(button(setup, "Install plugin")).toBeDisabled();
    await button(setup, "Install Vagrant").click();
    await expect(setup.getByText("vagrant was successfully installed!", { exact: false })).toBeVisible();
    await button(setup, "Install plugin").click();
    await expect(setup.getByText("Installed the plugin 'vagrant_utm (0.3.1)'!")).toBeVisible();
    await expect(button(setup, "Continue")).toBeEnabled();
    await button(setup, "Continue").click();

    await expect(stepTitle(setup)).toHaveText("Test VM labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    await expect(setup.getByText("Tested and ready to run.")).toHaveCount(2);
    const closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;
  });

  test("Fix opens the window at its step, a later Fix jumps there, and a failed install says why", async ({ page, context }) => {
    await openApp(page, "&machine=fresh&install=fail&os=macos&arch=arm64");
    await navButton(page, "Machine").click();
    const setup = await opens(context, () => page.locator("main").getByRole("button", { name: "Fix", exact: true }).first().click());
    await expect(setup).toHaveURL(/\/machine-setup\?step=docker&mock&machine=fresh&install=fail/);
    await expect(stepOf(setup)).toHaveText("Step 2 of 8");
    await expect(stepTitle(setup)).toHaveText("Container engine");

    // The first install fails: said in its log, and the step still waits.
    await button(setup, "Install Docker Desktop").click();
    await expect(setup.getByText("✗ docker: the installer exited with code 1 (dev mock)")).toBeVisible();
    await expect(button(setup, "Continue")).toBeDisabled();
    await button(setup, "Install Docker Desktop").click();
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toContainText("in use");

    // A Fix while the window is open: the core sends it the step.
    await emitEvent(setup, "machine-setup-step", "vagrant");
    await expect(stepTitle(setup)).toHaveText("Provisioning");
    await expect(stepOf(setup)).toHaveText("Step 6 of 8");
  });
});
