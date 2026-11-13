import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, release } from "./fixtures";

// The app signed out, past the onboarding (`auth=out`, see src/lib/dev-mock/scenario.ts): what the
// Labs list, a lab's page, the sidebar and Settings say, and how they all recover once the player
// signs in from Settings (its own window: the dev mock shares the sign-in between the tabs, as the
// core's one session is). Then the sign-in itself: its wait and the ways out of it, what asked for
// it going on once it is through (a lab's start, a row's lab), and a session that expired under
// the app being said.

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });
/** The app's sidebar (a lab page has an aside of its own, the side rail). */
const sidebar = (page: Page) => page.locator("aside").filter({ has: page.locator("nav") });

test.describe("signed out", () => {
  test("the list, a lab, the sidebar and Settings say so, and signing in from Settings brings them all back", async ({ page, context }) => {
    await openApp(page, "&auth=out");
    // The sidebar: Sign in in place of the account.
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(sidebar(page)).not.toContainText("Ada");

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
    await expect(settings.getByText("Ada Lovelace", { exact: true })).toBeVisible();
    await expect(settings.getByText("ada@example.org")).toBeVisible();
    await expect(settings.getByRole("button", { name: "Sign out" })).toBeVisible();
    await expect(settings.getByText("Online", { exact: true })).toBeVisible();
    await expect(settings.getByText("Not signed in")).toHaveCount(0);

    // The main window heard it: the account in the sidebar, Start lab on the lab, no banner.
    await expect(sidebar(page)).toContainText("Ada Lovelace");
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
    await expect(sidebar(page)).toContainText("Ada Lovelace");
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
  });

  test("the wait shows what to do and the ways out: the link to copy, a code instead, or Cancel", async ({ page }) => {
    await openApp(page, "&auth=out&hold=login");
    await sidebar(page).getByRole("button", { name: "Sign in", exact: true }).click();
    const wait = sidebar(page).getByText("Continue in your browser").locator("xpath=ancestor::div[1]");
    await expect(wait).toContainText("Sign in on cyberauth.co; the launcher comes back by itself once you're done.");
    await expect(wait.getByRole("button", { name: "Copy the sign-in link" })).toBeVisible();
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toHaveCount(0);
    // A code instead: the code, where to enter it, and how long it lasts.
    await wait.getByRole("button", { name: "Use a code instead" }).click();
    const code = sidebar(page).getByText("Sign in with a code").locator("xpath=ancestor::div[1]");
    await expect(code).toContainText("WDJB-MJHT");
    await expect(code).toContainText("Enter it at https://www.cyberauth.co/device on any device.");
    await expect(code).toContainText(/Expires in 1[45]:[0-5]\d/);
    await expect(code.getByRole("button", { name: "Copy the link with the code" })).toBeVisible();
    // Cancel: back to Sign in, nothing said. (A dispatched click: Next's dev badge sits over the
    // sidebar's bottom-left corner, where this button is, in the dev server the tests run against.)
    await code.getByRole("button", { name: "Cancel" }).dispatchEvent("click");
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(sidebar(page)).not.toContainText("expired");
    await expect(sidebar(page)).not.toContainText("Ada");
    // Signed in once the browser is done: said once, and the account takes the button's place.
    await sidebar(page).getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(sidebar(page).getByText("Continue in your browser")).toBeVisible();
    await release(page, "login");
    await expect(page.getByRole("status").filter({ hasText: "Signed in as Ada Lovelace" })).toBeVisible();
    await expect(sidebar(page)).toContainText("Ada Lovelace");
    // The account's picture, over the initials.
    await expect(sidebar(page).locator("img")).toHaveAttribute("src", /^data:image\/svg\+xml/);
  });

  test("signing in from a lab's header goes on into its start", async ({ page }) => {
    await openApp(page, "&auth=out");
    await navButton(page, "Labs").click();
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await button(page, "Sign in to start").click();
    // Signed in: the start the player asked for goes on, where to run asked first (there is a choice).
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where).toBeVisible();
    await expect(sidebar(page)).toContainText("Ada Lovelace");
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(where).toBeHidden();
    await expect(button(page, "Stop & remove")).toBeEnabled();
    await expect(button(page, "Sign in to start")).toHaveCount(0);
  });

  test("signing in from a lab's row on the Overview opens that lab", async ({ page }) => {
    await openApp(page, "&auth=out");
    // The row: the innermost div holding both the lab's title button and its Sign in.
    const row = main(page)
      .locator("div")
      .filter({ has: page.getByRole("button", { name: "Open Blind orders", exact: true }) })
      .filter({ has: page.getByRole("button", { name: "Sign in", exact: true }) })
      .last();
    await row.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Blind orders");
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(sidebar(page)).toContainText("Ada Lovelace");
  });

  test("a session that expired under the app is said, in the sidebar and in Settings, until the next sign-in", async ({ page, context }) => {
    await openApp(page, "&auth=expired");
    const why = "Your session expired. Sign in again to continue.";
    await expect(sidebar(page)).toContainText(why);
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    const [settings] = await Promise.all([
      context.waitForEvent("page"),
      sidebar(page)
        .getByRole("button", { name: /^Settings/ })
        .click(),
    ]);
    await settings.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: "Account" }).click();
    await expect(settings.getByText("Not signed in")).toBeVisible();
    await expect(settings.getByText(why)).toBeVisible();
    await settings.close();
    // Signed in again: the reason is gone with the sign-in.
    await sidebar(page).getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(sidebar(page)).toContainText("Ada Lovelace");
    await expect(sidebar(page)).not.toContainText(why);
  });
});
