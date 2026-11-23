import { test, expect, openApp, release } from "../fixtures";
import { SettingsWindow } from "../pages/settings";

// The app signed out, past the onboarding (`auth=out`, see src/lib/dev-mock/scenario.ts): what the
// Labs list, a lab's page, the sidebar and Settings say, and how they all recover once the player
// signs in from Settings (its own window: the dev mock shares the sign-in between the tabs, as the
// core's one session is). Then the sign-in itself: its wait and the ways out of it, what asked for
// it going on once it is through (a lab's start, a row's lab), and a session that expired under
// the app being said.

test.describe("signed out", () => {
  test("the list, a lab, the sidebar and Settings say so, and signing in from Settings brings them all back", async ({ page, app, context, labs }) => {
    await openApp(page, "&auth=out");
    // The sidebar: Sign in in place of the account.
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(app.sidebar).not.toContainText("Ada");

    // The Labs list: one banner above the labs, the count without what is solved (nothing is
    // tracked signed out), the labs still open.
    await app.nav("Labs").click();
    await expect(app.title).toHaveText("Labs");
    await expect(app.main).toContainText("Sign in to start labs");
    await expect(app.main).toContainText("Browse freely; signing in lets you start labs and keeps track of what you solve.");
    await expect(app.main).toContainText("6 in the catalogue · 1 running on this machine");
    await expect(app.main).not.toContainText("solved");
    await expect(labs.titleCell("Public backup")).not.toContainText("Solved");

    // A lab's page: Sign in to start, in place of Start lab.
    await labs.open("Blind orders").click();
    await expect(app.title).toHaveText("Blind orders");
    await expect(app.button("Sign in to start")).toBeVisible();
    await expect(app.button("Start lab")).toHaveCount(0);
    await expect(app.main).toContainText("Not started");

    // Settings, in its own window: the account row.
    const [settings] = await Promise.all([context.waitForEvent("page"), app.sidebar.getByRole("button", { name: /^Settings/ }).click()]);
    await new SettingsWindow(settings).show("Account");
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
    await expect(app.sidebar).toContainText("Ada Lovelace");
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toHaveCount(0);
    await expect(app.button("Start lab")).toBeEnabled();
    await expect(app.button("Sign in to start")).toHaveCount(0);
    await app.nav("Labs").click();
    await expect(app.main).not.toContainText("Sign in to start labs");
    await expect(app.main).toContainText("6 in the catalogue · 1 running on this machine · 1 solved");
  });

  test("signing in from the Labs list banner recovers the list and the lab", async ({ page, app, labs }) => {
    await openApp(page, "&auth=out");
    await app.nav("Labs").click();
    const banner = app.main.getByText("Sign in to start labs").locator("xpath=ancestor::div[.//button][1]");
    await banner.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(app.main).not.toContainText("Sign in to start labs");
    await expect(app.sidebar).toContainText("Ada Lovelace");
    await labs.open("Blind orders").click();
    await expect(app.button("Start lab")).toBeEnabled();
  });

  test("the wait shows what to do and the ways out: the link to copy, a code instead, or Cancel", async ({ page, app }) => {
    await openApp(page, "&auth=out&hold=login");
    await app.sidebar.getByRole("button", { name: "Sign in", exact: true }).click();
    const wait = app.sidebar.getByText("Continue in your browser").locator("xpath=ancestor::div[1]");
    await expect(wait).toContainText("Sign in on cyberauth.co; the launcher comes back by itself once you're done.");
    await expect(wait.getByRole("button", { name: "Copy the sign-in link" })).toBeVisible();
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toHaveCount(0);
    // A code instead: the code, where to enter it, and how long it lasts.
    await wait.getByRole("button", { name: "Use a code instead" }).click();
    const code = app.sidebar.getByText("Sign in with a code").locator("xpath=ancestor::div[1]");
    await expect(code).toContainText("WDJB-MJHT");
    await expect(code).toContainText("Enter it at https://www.cyberauth.co/device on any device.");
    await expect(code).toContainText(/Expires in 1[45]:[0-5]\d/);
    await expect(code.getByRole("button", { name: "Copy the link with the code" })).toBeVisible();
    // Cancel: back to Sign in, nothing said. (A dispatched click: Next's dev badge sits over the
    // sidebar's bottom-left corner, where this button is, in the dev server the tests run against.)
    await code.getByRole("button", { name: "Cancel" }).dispatchEvent("click");
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(app.sidebar).not.toContainText("expired");
    await expect(app.sidebar).not.toContainText("Ada");
    // Signed in once the browser is done: said once, and the account takes the button's place.
    await app.sidebar.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(app.sidebar.getByText("Continue in your browser")).toBeVisible();
    await release(page, "login");
    await expect(page.getByRole("status").filter({ hasText: "Signed in as Ada Lovelace" })).toBeVisible();
    await expect(app.sidebar).toContainText("Ada Lovelace");
    // The account's picture, over the initials.
    await expect(app.sidebar.locator("img")).toHaveAttribute("src", /^data:image\/svg\+xml/);
  });

  test("signing in from a lab's header goes on into its start", async ({ page, app, labs, lab }) => {
    await openApp(page, "&auth=out");
    await app.nav("Labs").click();
    await labs.open("Blind orders").click();
    await app.button("Sign in to start").click();
    // Signed in: the start the player asked for goes on, where to run asked first (there is a choice).
    const where = lab.whereToRun;
    await expect(where.dialog).toBeVisible();
    await expect(app.sidebar).toContainText("Ada Lovelace");
    await where.start();
    await expect(app.button("Stop & remove")).toBeEnabled();
    await expect(app.button("Sign in to start")).toHaveCount(0);
  });

  test("signing in from a lab's row on the Overview opens that lab", async ({ page, app }) => {
    await openApp(page, "&auth=out");
    // The row: the innermost div holding both the lab's title button and its Sign in.
    const row = app.main
      .locator("div")
      .filter({ has: page.getByRole("button", { name: "Open Blind orders", exact: true }) })
      .filter({ has: page.getByRole("button", { name: "Sign in", exact: true }) })
      .last();
    await row.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(app.title).toHaveText("Blind orders");
    await expect(app.button("Start lab")).toBeEnabled();
    await expect(app.sidebar).toContainText("Ada Lovelace");
  });

  test("a session that expired under the app is said, in the sidebar and in Settings, until the next sign-in", async ({ page, app, context }) => {
    await openApp(page, "&auth=expired");
    const why = "Your session expired. Sign in again to continue.";
    await expect(app.sidebar).toContainText(why);
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    const [settings] = await Promise.all([context.waitForEvent("page"), app.sidebar.getByRole("button", { name: /^Settings/ }).click()]);
    await new SettingsWindow(settings).show("Account");
    await expect(settings.getByText("Not signed in")).toBeVisible();
    await expect(settings.getByText(why)).toBeVisible();
    await settings.close();
    // Signed in again: the reason is gone with the sign-in.
    await app.sidebar.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(app.sidebar).toContainText("Ada Lovelace");
    await expect(app.sidebar).not.toContainText(why);
  });

  test("Sign out in Settings > Account forgets the session, and every window reads signed out", async ({ page, app, context }) => {
    await openApp(page);
    await expect(app.sidebar).toContainText("Ada Lovelace");
    const [settings] = await Promise.all([context.waitForEvent("page"), app.sidebar.getByRole("button", { name: /^Settings/ }).click()]);
    await new SettingsWindow(settings).show("Account");
    await expect(settings.getByText("Ada Lovelace", { exact: true })).toBeVisible();

    await settings.getByRole("button", { name: "Sign out", exact: true }).click();
    // The Settings window: no account, the sign-in offered again.
    await expect(settings.getByText("Not signed in")).toBeVisible();
    await expect(settings.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(settings.getByRole("button", { name: "Sign out" })).toHaveCount(0);
    // The main window, without a reload: the sidebar offers the sign-in, and starting a lab asks for it.
    await expect(app.sidebar).not.toContainText("Ada Lovelace");
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await settings.close();
    await app.nav("Labs").click();
    await app.button("Open Blind orders").click();
    await expect(app.button("Sign in to start")).toBeVisible();
  });
});
