import type { BrowserContext, Page } from "@playwright/test";
import { test, expect, openApp, navButton, emitEvent, release } from "./fixtures";
import { machineCheckInterval } from "../../src/features/machine/check-cadence";

// The guided machine-setup window (`/machine-setup`), opened as the app opens it: from the
// Overview's "Set up this machine" or a lab's "Set up", or at a step from a provider's "Fix" on the
// Machine page (the dev mock's `machine_open_setup` opens a tab, with `?step=`). The mock's
// one-click installs stream an installer's output and succeed, and the next system check reports
// the tool installed; `hold=pm` keeps the package manager missing until the test lets it show up
// (as if the player ran Homebrew's install command), `install=fail` fails the first install (see
// src/lib/dev-mock/scenario.ts).

/** How often the setup window checks the machine on its own while its checks answer. */
const CHECK_EVERY_MS = machineCheckInterval(false);

const stepOf = (page: Page) => page.getByText(/^Step \d+ of \d+$/);
const stepTitle = (page: Page) => page.getByRole("heading", { level: 3 });
const button = (page: Page, name: string) => page.getByRole("button", { name, exact: true });

/** A lab type's path on the window's first step ("What do you want to run?"): its name, how much of
 *  it is done, and its button (Set up, or Review once it is ready). */
const pathRow = (page: Page, name: string) =>
  page
    .locator("main div")
    .filter({ has: page.getByText(name, { exact: true }) })
    .filter({ has: page.getByRole("button") })
    .last();

/** The setup window the click opens. */
async function opens(context: BrowserContext, click: () => Promise<void>) {
  const [setup] = await Promise.all([context.waitForEvent("page"), click()]);
  await expect(setup.getByRole("heading", { level: 1, name: "Set up this machine" })).toBeVisible();
  return setup;
}

test.describe("machine setup window", () => {
  test("a ready machine: each lab type's path starts where the machine stands, Back walks it, and Done closes the window", async ({ page, context }) => {
    await openApp(page);
    // A ready machine shows no way into the whole flow (only a provider's Fix, at its step): the
    // window as the app opens it.
    let setup = await opens(context, () =>
      page.evaluate(() => {
        window.open("/machine-setup?mock");
      }),
    );
    await expect(setup).toHaveURL(/\/machine-setup\?mock$/);

    // One path per lab type, and how much of it this machine has done.
    await expect(stepTitle(setup)).toHaveText("What do you want to run?");
    await expect(pathRow(setup, "Container labs")).toContainText("3 of 4 done");
    await expect(pathRow(setup, "VM labs")).toContainText("3 of 4 done");

    // Container labs: the path starts at its test, which runs on its own.
    await pathRow(setup, "Container labs").getByRole("button").click();
    await expect(stepOf(setup)).toHaveText("Step 3 of 5");
    await expect(stepTitle(setup)).toHaveText("Test container labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    // Back walks the steps already done, then to the paths.
    await button(setup, "Back").click();
    await expect(stepTitle(setup)).toHaveText("Container engine");
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toHaveAttribute("aria-checked", "true");
    await expect(setup.getByRole("radio", { name: /^Docker Desktop/ })).toContainText("in use");
    await button(setup, "Back").click();
    await expect(stepOf(setup)).toHaveText("Step 1 of 5");
    await expect(stepTitle(setup)).toHaveText("Package manager");
    await expect(setup.getByText("brew", { exact: true })).toBeVisible();
    await button(setup, "Back").click();
    // Its test passed: the path reads ready, and Review opens its end.
    await expect(stepTitle(setup)).toHaveText("What do you want to run?");
    await expect(pathRow(setup, "Container labs")).toContainText("ready");
    await pathRow(setup, "Container labs").getByRole("button", { name: "Review", exact: true }).click();
    await expect(stepOf(setup)).toHaveText("Step 5 of 5");
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    await button(setup, "Back").click();
    await expect(stepTitle(setup)).toHaveText("Attack machine");
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    let closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;

    // VM labs: at their test too, after the hypervisor and Vagrant.
    setup = await opens(context, () =>
      page.evaluate(() => {
        window.open("/machine-setup?mock");
      }),
    );
    await pathRow(setup, "VM labs").getByRole("button").click();
    await expect(stepTitle(setup)).toHaveText("Test VM labs");
    await expect(setup.getByText("Passed")).toBeVisible();
    await button(setup, "Back").click();
    await expect(stepTitle(setup)).toHaveText("Provisioning");
    await expect(setup.getByText("2.4.3")).toBeVisible();
    await button(setup, "Back").click();
    await expect(stepTitle(setup)).toHaveText("Virtual machines");
    await expect(setup.getByRole("radio", { name: /^UTM/ })).toHaveAttribute("aria-checked", "true");
    await button(setup, "Continue").click();
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("Test VM labs");
    await button(setup, "Continue").click();
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;
  });

  test("a fresh Mac: the package manager is detected, then each install streams and the next check has it", async ({ page, context }) => {
    // The setup window checks the machine on its own every so often, on the test's clock: it runs
    // like the real one, and the test skips it ahead to the next check instead of waiting for it.
    await context.clock.install();
    await openApp(page, "&machine=fresh&hold=pm&os=macos&arch=arm64");
    // The Overview says to set it up.
    const setUp = page.locator("main").getByRole("button", { name: "Set up this machine" });
    const setup = await opens(context, () => setUp.click());
    await expect(pathRow(setup, "VM labs")).toContainText("0 of 4 done");

    // VM labs. Setup tools: Homebrew's command to run, and Continue waits until it's there.
    await pathRow(setup, "VM labs").getByRole("button").click();
    await expect(stepTitle(setup)).toHaveText("Package manager");
    await expect(setup.getByText(/raw\.githubusercontent\.com\/Homebrew\/install/)).toBeVisible();
    await expect(button(setup, "Continue")).toBeDisabled();
    // Homebrew installed (the setup window's own preview holds it): its next check has it.
    await release(setup, "pm");
    await context.clock.runFor(CHECK_EVERY_MS);
    await expect(button(setup, "Continue")).toBeEnabled();
    await expect(setup.getByText("brew", { exact: true })).toBeVisible();
    await button(setup, "Continue").click();

    // Virtual machines: UTM is the one Cyber CTF installs on Apple Silicon; its output streams.
    await expect(stepTitle(setup)).toHaveText("Virtual machines");
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
    const closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;
  });

  test("a fresh Mac: container labs install their engine, then test it", async ({ page, context }) => {
    await context.clock.install();
    await openApp(page, "&machine=fresh&hold=pm&os=macos&arch=arm64");
    const setup = await opens(context, () => page.locator("main").getByRole("button", { name: "Set up this machine" }).click());
    await expect(pathRow(setup, "Container labs")).toContainText("of 4 done");
    await pathRow(setup, "Container labs").getByRole("button").click();
    // The package manager first (Homebrew run by the player; the window's next check has it).
    await expect(stepTitle(setup)).toHaveText("Package manager");
    await release(setup, "pm");
    await context.clock.runFor(CHECK_EVERY_MS);
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
    await expect(stepTitle(setup)).toHaveText("You’re set up");
    const closed = setup.waitForEvent("close");
    await button(setup, "Done").click();
    await closed;
  });

  test("Fix opens the window at its step, a later Fix jumps there, and a failed install says why", async ({ page, context }) => {
    // VMware Fusion is installed, short of its Vagrant plugin (`machine=many`): its row's Fix.
    await openApp(page, "&machine=many&install=fail");
    await navButton(page, "Machine").click();
    const fusion = page.locator("main").getByRole("list", { name: "Providers" }).getByRole("listitem").filter({ hasText: "VMware Fusion" });
    await expect(fusion).toContainText("Needs setup");
    const setup = await opens(context, () => fusion.getByRole("button", { name: "Fix", exact: true }).click());
    await expect(setup).toHaveURL(/\/machine-setup\?step=vagrant&provider=vmware_desktop&mock&machine=many&install=fail/);
    await expect(stepTitle(setup)).toHaveText("Provisioning");

    // The first install fails: said in its log, and the step still waits.
    await button(setup, "Install plugin").click();
    await expect(setup.getByText("✗ vagrant-vmware-desktop: the installer exited with code 1 (dev mock)")).toBeVisible();
    await expect(button(setup, "Continue")).toBeDisabled();
    await button(setup, "Install plugin").click();
    await expect(setup.getByText("Installed the plugin 'vagrant-vmware-desktop (0.3.1)'!")).toBeVisible();

    // A Fix while the window is open: the core sends it the step.
    await emitEvent(setup, "machine-setup-step", { step: "docker", provider: null });
    await expect(stepTitle(setup)).toHaveText("Container engine");
  });
});
