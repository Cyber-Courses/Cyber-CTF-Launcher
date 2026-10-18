import type { Page } from "@playwright/test";
import { test, expect, navButton, pageTitle, release } from "./fixtures";

// The first-run onboarding, on each machine the dev mock can report (`?mock&onboarding` plus the
// scenario switches `os`, `arch` and `machine`, see src/lib/dev-mock/scenario.ts).

/** The step position line under the mark: "3 / 10 · Package manager". */
const progress = (page: Page) => page.getByText(/^\d+ \/ \d+ · /);

/** A machine step's forward button: the one right after Back. */
const forward = (page: Page) => page.getByRole("button", { name: "Back", exact: true }).locator("xpath=following::button[1]");

const onboarded = (page: Page) => page.evaluate(() => localStorage.getItem("cyberctf.onboarded"));

/** The steps after the sign-in on a ready machine, by their progress name. */
const MACHINE_STEPS = [
  "Package manager",
  "Container engine",
  "Container test",
  "Attack box",
  "Virtual machines (optional)",
  "Vagrant (optional)",
  "VM test (optional)",
];

const READY = [
  { name: "macOS on Apple Silicon", query: "&os=macos&arch=arm64", wsl: false, intel: false },
  { name: "macOS on Intel", query: "&os=macos&arch=x86_64", wsl: false, intel: true },
  { name: "Windows", query: "&os=windows&arch=x86_64", wsl: true, intel: false },
  { name: "Linux", query: "&os=linux&arch=x86_64", wsl: false, intel: false },
];

test.describe("onboarding", () => {
  for (const machine of READY) {
    test(`a ready ${machine.name} machine goes from the welcome to "You're all set"`, async ({ page }) => {
      const steps = ["Welcome", "Sign in", ...MACHINE_STEPS.slice(0, 1), ...(machine.wsl ? ["Virtualization"] : []), ...MACHINE_STEPS.slice(1), "Done"];
      const total = steps.length;
      await page.goto(`/?mock&onboarding${machine.query}`);
      await expect(page.getByRole("heading", { level: 1, name: "Welcome to Cyber CTF" })).toBeVisible();
      await expect(progress(page)).toHaveText(`1 / ${total} · Welcome`);
      // No shell behind it.
      await expect(page.locator("aside nav")).toHaveCount(0);

      await page.getByRole("button", { name: "Get started" }).click();
      await expect(progress(page)).toHaveText(`2 / ${total} · Sign in`);
      await expect(page.getByText("Signed in as Florian Amette")).toBeVisible();
      await forward(page).click();

      for (let n = 2; n < total - 1; n++) {
        await expect(progress(page)).toHaveText(`${n + 1} / ${total} · ${steps[n]}`);
        if (steps[n] === "Package manager") {
          const intelNote = page.getByText(/This is an Intel Mac/);
          if (machine.intel) await expect(intelNote).toBeVisible();
          else await expect(intelNote).toHaveCount(0);
        }
        // Docker runs on a ready machine, so WSL 2 works: nothing to turn on, no restart (#224).
        if (steps[n] === "Virtualization") {
          await expect(page.getByText("On and running")).toBeVisible();
          await expect(page.getByRole("button", { name: "Turn on WSL 2" })).toHaveCount(0);
        }
        // The self-tests run on their own; the flow waits for them.
        if (steps[n].endsWith("test") || steps[n].startsWith("VM test")) await expect(page.getByText("Passed")).toBeVisible();
        await expect(forward(page)).toBeEnabled();
        await forward(page).click();
      }

      await expect(progress(page)).toHaveText(`${total} / ${total} · Done`);
      await expect(page.getByRole("heading", { level: 1, name: "You’re all set" })).toBeVisible();
      // Both tests passed on the way.
      await expect(page.getByText("Tested and ready to run.")).toHaveCount(2);
      // The last step has no "Skip setup": it is the way out.
      await expect(page.getByRole("button", { name: "Skip setup" })).toHaveCount(0);

      await page.getByRole("button", { name: "Browse labs" }).click();
      await expect(pageTitle(page)).toHaveText("Labs");
      await expect(navButton(page, "Labs")).toHaveAttribute("aria-current", "page");
      expect(await onboarded(page)).toBe("1");
    });
  }

  test("Back returns to the previous step and keeps the flow's place", async ({ page }) => {
    await page.goto("/?mock&onboarding");
    await page.getByRole("button", { name: "Get started" }).click();
    await forward(page).click();
    await expect(progress(page)).toHaveText("3 / 10 · Package manager");
    await forward(page).click();
    await expect(progress(page)).toHaveText("4 / 10 · Container engine");
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(progress(page)).toHaveText("3 / 10 · Package manager");
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(progress(page)).toHaveText("2 / 10 · Sign in");
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(progress(page)).toHaveText("1 / 10 · Welcome");
  });

  const FRESH = [
    { os: "macos", query: "&os=macos&arch=x86_64", instructions: (page: Page) => page.getByText(/raw\.githubusercontent\.com\/Homebrew\/install/) },
    { os: "windows", query: "&os=windows&arch=x86_64", instructions: (page: Page) => page.getByText("App Installer (winget)") },
    { os: "linux", query: "&os=linux&arch=x86_64", instructions: (page: Page) => page.getByText(/package manager \(apt\)/) },
  ];
  for (const machine of FRESH) {
    test(`a fresh ${machine.os} machine waits on its package manager, and Skip setup lands on Labs`, async ({ page }) => {
      await page.goto(`/?mock&onboarding&machine=fresh${machine.query}`);
      await page.getByRole("button", { name: "Get started" }).click();
      await forward(page).click();
      await expect(progress(page)).toHaveText(/^3 \/ \d+ · Package manager$/);
      await expect(machine.instructions(page)).toBeVisible();
      // Nothing to continue with until the package manager is there.
      await expect(forward(page)).toBeDisabled();

      await page.getByRole("button", { name: "Skip setup" }).click();
      await expect(pageTitle(page)).toHaveText("Labs");
      expect(await onboarded(page)).toBe("1");

      // The Machine page says what is left to do.
      await navButton(page, "Machine").click();
      await expect(page.getByText("2 lab types need setup")).toBeVisible();
      await expect(page.getByRole("button", { name: "Set up this machine" })).toBeVisible();
    });
  }

  test("signing in can be skipped for now: the summary and the app say so", async ({ page }) => {
    await page.goto("/?mock&onboarding&auth=out");
    await page.getByRole("button", { name: "Get started" }).click();
    await expect(page.getByText(/sign in later from Settings/)).toBeVisible();
    await page.getByRole("button", { name: "Skip for now" }).click();
    await expect(progress(page)).toHaveText("3 / 10 · Package manager");
    for (let n = 0; n < 7; n++) {
      await expect(forward(page)).toBeEnabled();
      await forward(page).click();
    }
    await expect(page.getByRole("heading", { level: 1, name: "You’re all set" })).toBeVisible();
    await expect(page.getByText("Not signed in (sign in later from Settings)")).toBeVisible();
    await page.getByRole("button", { name: "Browse labs" }).click();
    // The app opens signed out, the labs listed, with the way to sign in still there.
    await expect(pageTitle(page)).toHaveText("Labs");
    await expect(
      page
        .locator("main")
        .getByRole("button", { name: /^Open / })
        .first(),
    ).toBeVisible();
    expect(await onboarded(page)).toBe("1");
  });

  test("signing in from the onboarding shows on the summary", async ({ page }) => {
    await page.goto("/?mock&onboarding&auth=out");
    await page.getByRole("button", { name: "Get started" }).click();
    await page.getByRole("button", { name: "Sign in with your browser" }).click();
    await expect(page.getByText("Signed in as Florian Amette")).toBeVisible();
    for (let n = 0; n < 8; n++) {
      await expect(forward(page)).toBeEnabled();
      await forward(page).click();
    }
    await expect(page.getByRole("heading", { level: 1, name: "You’re all set" })).toBeVisible();
    await expect(page.getByText("Signed in as Florian Amette")).toBeVisible();
    await page.getByRole("button", { name: "Browse labs" }).click();
    // The shell picked the sign-in up: the account shows in the sidebar, no "Sign in" prompt.
    await expect(pageTitle(page)).toHaveText("Labs");
    await expect(page.locator("aside")).toContainText("Florian");
  });

  test("a failed machine check during onboarding says so, and Try again recovers", async ({ page }) => {
    // Every check fails (`hold=check`: the shell's, the onboarding's at mount and its polls) until
    // the machine comes back with the Try again.
    await page.goto("/?mock&onboarding&hold=check");
    await page.getByRole("button", { name: "Get started" }).click();
    await forward(page).click();
    await expect(page.getByText("Couldn’t check this machine")).toBeVisible();
    await expect(page.getByText("docker info timed out after 20 s")).toBeVisible();
    await release(page, "check", page.getByRole("button", { name: "Try again" }));
    await expect(page.getByRole("heading", { level: 1, name: "Package manager" })).toBeVisible();
    await expect(forward(page)).toBeEnabled();
  });

  test("Replay in Settings shows the onboarding again in the main window", async ({ page, context }) => {
    await page.goto("/?mock");
    await expect(page.locator("aside nav")).toBeVisible();
    const [settings] = await Promise.all([
      context.waitForEvent("page"),
      page
        .locator("aside")
        .getByRole("button", { name: /^Settings/ })
        .click(),
    ]);
    await settings.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: "About" }).click();
    await settings.getByRole("button", { name: "Replay" }).click();

    await expect(page.getByRole("heading", { level: 1, name: "Welcome to Cyber CTF" })).toBeVisible();
    await expect(page.locator("aside nav")).toHaveCount(0);
    expect(await onboarded(page)).toBeNull();

    // Skipping it again brings the shell back, on Labs.
    await page.getByRole("button", { name: "Skip setup" }).click();
    await expect(pageTitle(page)).toHaveText("Labs");
    expect(await onboarded(page)).toBe("1");
  });
});
