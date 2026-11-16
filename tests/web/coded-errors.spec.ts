import { test, expect, openApp, forceCommands, reforce } from "./fixtures";
import type { LabScreen } from "./pages/lab";

// The Rust core's coded errors (src/lib/tauri/generated/CommandError.ts), each driven through the
// real screens: a lab start forced to reject with the code (`forceCommands`), the dev mock
// answering everything else. What the player sees is the code's translated message
// (src/messages/en/errors.json `codes`), the hint for its kind of failure, and what the code
// changes beyond the message: the retry on random ports, the offline-start prompt, the signed-out
// state, a refused start that keeps the previous run. `reforce` lets the next start go through
// (or fail another way), as the core would once the cause is gone.

/** The failure said under the lab's header (the toast, outside `main`, says it too). */
const FAILED_START = "Couldn’t start Blind orders";

const SIGNED_OUT = { loggedIn: false, name: null, email: null };

/** Start lab (or Retry), then Start in "Where should it run?" on this machine. */
async function startHere(lab: LabScreen, from: "Start lab" | "Retry" = "Start lab") {
  const where = await lab.askWhere(from);
  await where.expectPicked(where.containersHere);
  await where.start();
}

test.describe("coded errors", () => {
  test("a port in use: the port named, the hint, and Retry with random ports starts the lab", async ({ page, app, labs, lab }) => {
    await forceCommands(page, { lab_launch: { error: { code: "portInUse", args: { port: 8000 }, message: "port 8000 is in use" } } });
    // The lab's own ports, so a clash is what the player asked for.
    await page.addInitScript(() => localStorage.setItem("cyberctf.labs.ports", "default"));
    await openApp(page);
    await labs.openLab("Blind orders");
    await startHere(lab);

    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice(FAILED_START)).toContainText(
      "Host port 8000 is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again.",
    );
    await expect(lab.notice(FAILED_START)).toContainText(
      "A port this lab publishes is taken on this machine. Stop whatever uses it, or start it with random ports if the lab doesn't need that exact port.",
    );
    // Said in a toast too.
    await expect(page.getByRole("alert").filter({ hasText: "Couldn’t start Blind orders" })).toHaveCount(2);

    // The port freed (the core answers again): the same start on random ports goes through.
    await reforce(page, { lab_launch: null });
    await app.button("Retry with random ports").click();
    await expect(app.button("Stop & remove")).toBeEnabled();
    await expect(app.main).not.toContainText("Failed to start");
  });

  test("Cyber CTF unreachable with the lab installed here: declining the offline start keeps the failure", async ({ page, app, labs, lab }) => {
    await forceCommands(page, { lab_launch: { error: { code: "offlineReady", args: { detail: "connection refused" }, message: "offline" } } });
    await openApp(page);
    await labs.openLab("Blind orders");
    await startHere(lab);

    const prompt = page.getByRole("alertdialog", { name: "Start Blind orders offline?" });
    await expect(prompt).toContainText(
      "Cyber CTF can't be reached right now. This lab is installed on this machine, so it can start without it, but your progress and evidence aren't saved until you start it again online.",
    );
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(prompt).toBeHidden();

    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice(FAILED_START)).toContainText(
      "Cyber CTF can't be reached (connection refused). This lab is installed here, so it can start offline, without saving progress.",
    );
    // The detail reads as a network failure: its hint.
    await expect(lab.notice(FAILED_START)).toContainText(
      "Something it downloads, or the Cyber CTF API, couldn’t be reached. Check your internet connection, then retry.",
    );
    await expect(app.button("Retry")).toBeVisible();
  });

  test("Cyber CTF unreachable with the lab installed here: accepting starts it offline", async ({ page, app, labs, lab }) => {
    await forceCommands(page, { lab_launch: { error: { code: "offlineReady", args: { detail: "connection refused" }, message: "offline" } } });
    await openApp(page);
    await labs.openLab("Blind orders");
    await startHere(lab);

    const prompt = page.getByRole("alertdialog", { name: "Start Blind orders offline?" });
    await expect(prompt).toBeVisible();
    // The second start, offline, reaches the core (the mock): it starts the lab and says so.
    await reforce(page, { lab_launch: null });
    await prompt.getByRole("button", { name: "Start offline" }).click();
    await expect(prompt).toBeHidden();
    await expect(app.button("Stop & remove")).toBeEnabled();
    await expect(app.main).not.toContainText("Failed to start");
    await app.button("Show full log").click();
    await expect(app.main).toContainText("Starting offline: Cyber CTF can't be reached, progress isn't saved");
  });

  test("Docker not running: the header, the list's readiness, the dialog, and the start's message", async ({ page, app, labs, lab }) => {
    await forceCommands(page, { lab_launch: { error: { code: "dockerNotRunning", message: "docker isn't running" } } });
    await openApp(page, "&docker=stopped");
    await expect(app.header).toContainText("Docker not running");
    await app.nav("Labs").click();
    const row = labs.open("Blind orders");
    await expect(row.locator("xpath=..")).toContainText("Start your container engine");
    await row.click();
    await expect(app.title).toHaveText("Blind orders");

    const where = await lab.askWhere();
    await expect(where.containersHere).toContainText("Engine stopped");
    await where.start();

    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice(FAILED_START)).toContainText(
      "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.",
    );
    await expect(lab.notice(FAILED_START)).toContainText("Docker isn’t running. Start your container engine, then retry.");
  });

  test("a start that finds the session gone: the lab, the list and the sidebar read signed out, and Sign in brings it back", async ({
    page,
    app,
    labs,
    lab,
  }) => {
    await forceCommands(page, { lab_launch: { error: { code: "signedOut", message: "no session" } } });
    await openApp(page);
    await labs.openLab("Blind orders");
    await expect(app.sidebar).toContainText("Ada");
    // The session is gone from here on: the core answers so once the start found out.
    await reforce(page, { auth_status: { value: SIGNED_OUT } });
    await startHere(lab);

    await expect(lab.notice(FAILED_START)).toContainText("You're signed out. Sign in again to continue.");
    await expect(lab.notice(FAILED_START)).toContainText("Your session ended. Sign in again, then retry.");
    // The shell read the session again: signed out everywhere.
    await expect(app.button("Sign in to start")).toBeVisible();
    await expect(app.button("Start lab")).toHaveCount(0);
    await expect(app.sidebar.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
    await expect(app.sidebar).not.toContainText("Ada");
    await app.nav("Labs").click();
    await expect(app.main).toContainText("Sign in to start labs");
    await expect(app.main).toContainText("6 in the catalogue · 1 running on this machine");

    // Signing in again, from the lab's own header: the session is back, and the start the player
    // asked for goes on by itself (where to run asked first, as Retry would), and goes through.
    await reforce(page, { auth_status: null, lab_launch: null });
    await labs.open("Blind orders").click();
    await app.button("Sign in to start").click();
    await expect(app.sidebar).toContainText("Ada");
    const where = lab.whereToRun;
    await expect(where.containersHere).toHaveAttribute("aria-checked", "true");
    await where.start();
    await expect(app.button("Stop & remove")).toBeEnabled();
  });

  test("a command that failed, then a start refused while another operation runs: the toast, and the previous run kept", async ({ page, app, lab, labs }) => {
    await forceCommands(page, {
      lab_launch: { error: { code: "commandFailed", args: { command: "docker compose up", stderr: "exit status 125" }, message: "compose failed" } },
    });
    await openApp(page);
    await labs.openLab("Blind orders");
    await startHere(lab);
    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice(FAILED_START)).toContainText("`docker compose up` failed: exit status 125");

    // The backend refuses the retry: another operation is in flight on the lab.
    await reforce(page, { lab_launch: { error: { code: "alreadyInProgress", message: "busy" } } });
    await startHere(lab, "Retry");
    const refused = page.getByRole("alert").filter({ hasText: "Another operation is still running on this lab" });
    await expect(refused).toBeVisible();
    await expect(refused).toContainText("Another operation on this lab is already in progress; wait for it to finish.");
    // Nothing ran, so nothing failed anew: the previous failure stays as it was.
    await expect(app.main).toContainText("Failed to start");
    await expect(lab.notice(FAILED_START)).toContainText("`docker compose up` failed: exit status 125");
    await expect(lab.notice(FAILED_START)).not.toContainText("already in progress");
    await expect(app.main).not.toContainText("✗ Another operation");
    await expect(app.button("Retry")).toBeEnabled();
  });

  test("an expired session and a missing tool say their translated messages, with their hints", async ({ page, app, labs, lab }) => {
    await forceCommands(page, { lab_launch: { error: { code: "sessionExpired", message: "session expired" } } });
    await openApp(page);
    await labs.openLab("Blind orders");
    await startHere(lab);
    await expect(lab.notice(FAILED_START)).toContainText("Your session expired: you're signed out. Sign in again to continue.");
    await expect(lab.notice(FAILED_START)).toContainText("Your session ended. Sign in again, then retry.");
    // The core still has the session (the mock answers it): Retry stays the next step.
    await expect(app.button("Retry")).toBeVisible();

    await reforce(page, { lab_launch: { error: { code: "toolMissing", args: { tool: "vagrant" }, message: "vagrant missing" } } });
    await startHere(lab, "Retry");
    await expect(lab.notice(FAILED_START)).toContainText("`vagrant` is not installed or not on PATH");
    await expect(lab.notice(FAILED_START)).toContainText("A tool this lab needs is missing here. Install it from the Machine page, then retry.");
    await expect(lab.notice(FAILED_START)).not.toContainText("Your session expired");
  });
});
