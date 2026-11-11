import type { Locator, Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, forceCommands, reforce } from "./fixtures";

// The Rust core's coded errors (src/lib/tauri/generated/CommandError.ts), each driven through the
// real screens: a lab start forced to reject with the code (`forceCommands`), the dev mock
// answering everything else. What the player sees is the code's translated message
// (src/messages/en/errors.json `codes`), the hint for its kind of failure, and what the code
// changes beyond the message: the retry on random ports, the offline-start prompt, the signed-out
// state, a refused start that keeps the previous run. `reforce` lets the next start go through
// (or fail another way), as the core would once the cause is gone.

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });
const header = (page: Page) => page.locator("main > [data-tauri-drag-region]");
/** The app's sidebar (a lab page has an aside of its own, the side rail). */
const sidebar = (page: Page) => page.locator("aside").filter({ has: page.locator("nav") });
/** The failure said under the lab's header (the toast, outside `main`, says it too). */
const notice = (page: Page) => main(page).getByRole("alert").filter({ hasText: "Couldn’t start Blind orders" });

const SIGNED_OUT = { loggedIn: false, name: null, email: null };

/** "Where should it run?"'s row for containers on this machine. */
const containersHere = (where: Locator) => where.getByRole("radiogroup", { name: "This machine" }).getByRole("radio", { name: /^Container/ });

async function openBlindOrders(page: Page) {
  await navButton(page, "Labs").click();
  await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
  await expect(pageTitle(page)).toHaveText("Blind orders");
}

/** Start lab (or Retry), then Start in "Where should it run?" on this machine. */
async function startHere(page: Page, from: "Start lab" | "Retry" = "Start lab") {
  await button(page, from).click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  await expect(containersHere(where)).toHaveAttribute("aria-checked", "true");
  await where.getByRole("button", { name: "Start", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("coded errors", () => {
  test("a port in use: the port named, the hint, and Retry with random ports starts the lab", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "portInUse", args: { port: 8000 }, message: "port 8000 is in use" } } });
    // The lab's own ports, so a clash is what the player asked for.
    await page.addInitScript(() => localStorage.setItem("cyberctf.labs.ports", "default"));
    await openApp(page);
    await openBlindOrders(page);
    await startHere(page);

    await expect(main(page)).toContainText("Failed to start");
    await expect(notice(page)).toContainText(
      "Host port 8000 is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again.",
    );
    await expect(notice(page)).toContainText(
      "A port this lab publishes is taken on this machine. Stop whatever uses it, or start it with random ports if the lab doesn't need that exact port.",
    );
    // Said in a toast too.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start Blind orders" })).toHaveCount(2);

    // The port freed (the core answers again): the same start on random ports goes through.
    await reforce(page, { lab_launch: null });
    await button(page, "Retry with random ports").click();
    await expect(button(page, "Stop & remove")).toBeEnabled();
    await expect(main(page)).not.toContainText("Failed to start");
  });

  test("Cyber CTF unreachable with the lab installed here: declining the offline start keeps the failure", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "offlineReady", args: { detail: "connection refused" }, message: "offline" } } });
    await openApp(page);
    await openBlindOrders(page);
    await startHere(page);

    const prompt = page.getByRole("alertdialog", { name: "Start Blind orders offline?" });
    await expect(prompt).toContainText(
      "Cyber CTF can't be reached right now. This lab is installed on this machine, so it can start without it, but your progress and evidence aren't saved until you start it again online.",
    );
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(prompt).toBeHidden();

    await expect(main(page)).toContainText("Failed to start");
    await expect(notice(page)).toContainText(
      "Cyber CTF can't be reached (connection refused). This lab is installed here, so it can start offline, without saving progress.",
    );
    // The detail reads as a network failure: its hint.
    await expect(notice(page)).toContainText("Something it downloads, or the Cyber CTF API, couldn’t be reached. Check your internet connection, then retry.");
    await expect(button(page, "Retry")).toBeVisible();
  });

  test("Cyber CTF unreachable with the lab installed here: accepting starts it offline", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "offlineReady", args: { detail: "connection refused" }, message: "offline" } } });
    await openApp(page);
    await openBlindOrders(page);
    await startHere(page);

    const prompt = page.getByRole("alertdialog", { name: "Start Blind orders offline?" });
    await expect(prompt).toBeVisible();
    // The second start, offline, reaches the core (the mock): it starts the lab and says so.
    await reforce(page, { lab_launch: null });
    await prompt.getByRole("button", { name: "Start offline" }).click();
    await expect(prompt).toBeHidden();
    await expect(button(page, "Stop & remove")).toBeEnabled();
    await expect(main(page)).not.toContainText("Failed to start");
    await button(page, "Show full log").click();
    await expect(main(page)).toContainText("Starting offline: Cyber CTF can't be reached, progress isn't saved");
  });

  test("Docker not running: the header, the list's readiness, the dialog, and the start's message", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "dockerNotRunning", message: "docker isn't running" } } });
    await openApp(page, "&docker=stopped");
    await expect(header(page)).toContainText("Docker not running");
    await navButton(page, "Labs").click();
    const row = main(page).getByRole("button", { name: "Open Blind orders", exact: true });
    await expect(row.locator("xpath=..")).toContainText("Start your container engine");
    await row.click();
    await expect(pageTitle(page)).toHaveText("Blind orders");

    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(containersHere(where)).toContainText("Engine stopped");
    await where.getByRole("button", { name: "Start", exact: true }).click();

    await expect(main(page)).toContainText("Failed to start");
    await expect(notice(page)).toContainText(
      "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.",
    );
    await expect(notice(page)).toContainText("Docker isn’t running. Start your container engine, then retry.");
  });

  test("a start that finds the session gone: the lab, the list and the sidebar read signed out, and Sign in brings it back", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "signedOut", message: "no session" } } });
    await openApp(page);
    await openBlindOrders(page);
    await expect(sidebar(page)).toContainText("Florian");
    // The session is gone from here on: the core answers so once the start found out.
    await reforce(page, { auth_status: { value: SIGNED_OUT } });
    await startHere(page);

    await expect(notice(page)).toContainText("You're signed out. Sign in again to continue.");
    await expect(notice(page)).toContainText("Your session ended. Sign in again, then retry.");
    // The shell read the session again: signed out everywhere.
    await expect(button(page, "Sign in to start")).toBeVisible();
    await expect(button(page, "Start lab")).toHaveCount(0);
    await expect(sidebar(page).getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(sidebar(page)).not.toContainText("Florian");
    await navButton(page, "Labs").click();
    await expect(main(page)).toContainText("Sign in to start labs");
    await expect(main(page)).toContainText("6 in the catalogue · 1 running on this machine");

    // Signing in again, from the lab's own header: the session is back, and the start the player
    // asked for goes on by itself (where to run asked first, as Retry would), and goes through.
    await reforce(page, { auth_status: null, lab_launch: null });
    await main(page).getByRole("button", { name: "Open Blind orders", exact: true }).click();
    await button(page, "Sign in to start").click();
    await expect(sidebar(page)).toContainText("Florian");
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(containersHere(where)).toHaveAttribute("aria-checked", "true");
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(where).toBeHidden();
    await expect(button(page, "Stop & remove")).toBeEnabled();
  });

  test("a command that failed, then a start refused while another operation runs: the toast, and the previous run kept", async ({ page }) => {
    await forceCommands(page, {
      lab_launch: { error: { code: "commandFailed", args: { command: "docker compose up", stderr: "exit status 125" }, message: "compose failed" } },
    });
    await openApp(page);
    await openBlindOrders(page);
    await startHere(page);
    await expect(main(page)).toContainText("Failed to start");
    await expect(notice(page)).toContainText("`docker compose up` failed: exit status 125");

    // The backend refuses the retry: another operation is in flight on the lab.
    await reforce(page, { lab_launch: { error: { code: "alreadyInProgress", message: "busy" } } });
    await startHere(page, "Retry");
    const refused = page.getByRole("alert").filter({ hasText: "Another operation is still running on this lab" });
    await expect(refused).toBeVisible();
    await expect(refused).toContainText("Another operation on this lab is already in progress; wait for it to finish.");
    // Nothing ran, so nothing failed anew: the previous failure stays as it was.
    await expect(main(page)).toContainText("Failed to start");
    await expect(notice(page)).toContainText("`docker compose up` failed: exit status 125");
    await expect(notice(page)).not.toContainText("already in progress");
    await expect(main(page)).not.toContainText("✗ Another operation");
    await expect(button(page, "Retry")).toBeEnabled();
  });

  test("an expired session and a missing tool say their translated messages, with their hints", async ({ page }) => {
    await forceCommands(page, { lab_launch: { error: { code: "sessionExpired", message: "session expired" } } });
    await openApp(page);
    await openBlindOrders(page);
    await startHere(page);
    await expect(notice(page)).toContainText("Your session expired: you're signed out. Sign in again to continue.");
    await expect(notice(page)).toContainText("Your session ended. Sign in again, then retry.");
    // The core still has the session (the mock answers it): Retry stays the next step.
    await expect(button(page, "Retry")).toBeVisible();

    await reforce(page, { lab_launch: { error: { code: "toolMissing", args: { tool: "vagrant" }, message: "vagrant missing" } } });
    await startHere(page, "Retry");
    await expect(notice(page)).toContainText("`vagrant` is not installed or not on PATH");
    await expect(notice(page)).toContainText("A tool this lab needs is missing here. Install it from the Machine page, then retry.");
    await expect(notice(page)).not.toContainText("Your session expired");
  });
});
