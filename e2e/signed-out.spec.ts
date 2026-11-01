import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// The app signed out, past the onboarding (`auth=out`, see src/lib/dev-mock/scenario.ts): what the
// Labs list, a lab's page, the sidebar and Settings say, and how they all recover once the player
// signs in from Settings (its own window: the dev mock shares the sign-in between the tabs, as the
// core's one session is).

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });
/** The app's sidebar (a lab page has an aside of its own, the side rail). */
const sidebar = (page: Page) => page.locator("aside").filter({ has: page.locator("nav") });

test.describe("signed out", () => {
  test("the list, a lab, the sidebar and Settings say so, and signing in from Settings brings them all back", async ({ page, context }) => {
    await openApp(page, "&auth=out");
    // The sidebar: Sign in in place of the account.
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(sidebar(page)).not.toContainText("Florian");

    // The Labs list: one banner above the labs, the count without what is solved (nothing is
    // tracked signed out), the labs still open.
    await navButton(page, "Labs").click();
    await expect(pageTitle(page)).toHaveText("Labs");
    await expect(main(page)).toContainText("Sign in to start labs");
    await expect(main(page)).toContainText("Browse freely; signing in lets you start labs and keeps track of what you solve.");
    await expect(main(page)).toContainText("6 in the catalogue · 1 running on this machine");
    await expect(main(page)).not.toContainText("solved");
    await expect(main(page).getByRole("button", { name: "Open Public backup", exact: true }).locator("xpath=..")).not.toContainText("Solved");

    // A lab's page: Sign in to start, in place of Start lab.
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Blind orders");
    await expect(button(page, "Sign in to start")).toBeVisible();
    await expect(button(page, "Start lab")).toHaveCount(0);
    await expect(main(page)).toContainText("Not started");

    // Settings, in its own window: the account row.
    const [settings] = await Promise.all([
      context.waitForEvent("page"),
      sidebar(page)
        .getByRole("button", { name: /^Settings/ })
        .click(),
    ]);
    await settings.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: "Account" }).click();
    await expect(settings.getByText("Not signed in")).toBeVisible();
    await expect(settings.getByText("Offline", { exact: true })).toBeVisible();
    await expect(settings.getByText("Sign in with your Cyber account to sync labs and run them from any device.")).toBeVisible();
    await expect(settings.getByRole("button", { name: "Sign out" })).toHaveCount(0);

    // Sign in there: Settings shows the account and this machine online.
    await settings.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(settings.getByText("Florian Amette")).toBeVisible();
    await expect(settings.getByText("florian@cyberctf.org")).toBeVisible();
    await expect(settings.getByRole("button", { name: "Sign out" })).toBeVisible();
    await expect(settings.getByText("Online", { exact: true })).toBeVisible();
    await expect(settings.getByText("Not signed in")).toHaveCount(0);

    // The main window heard it: the account in the sidebar, Start lab on the lab, no banner.
    await expect(sidebar(page)).toContainText("Florian Amette");
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toHaveCount(0);
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(button(page, "Sign in to start")).toHaveCount(0);
    await navButton(page, "Labs").click();
    await expect(main(page)).not.toContainText("Sign in to start labs");
    await expect(main(page)).toContainText("6 in the catalogue · 1 running on this machine · 1 solved");
  });

  test("signing in from the Labs list banner recovers the list and the lab", async ({ page }) => {
    await openApp(page, "&auth=out");
    await navButton(page, "Labs").click();
    const banner = main(page).getByText("Sign in to start labs").locator("xpath=ancestor::div[.//button][1]");
    await banner.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(main(page)).not.toContainText("Sign in to start labs");
    await expect(sidebar(page)).toContainText("Florian Amette");
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
  });
});
