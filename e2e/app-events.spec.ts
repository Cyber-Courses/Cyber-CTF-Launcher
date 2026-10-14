import { test, expect, openApp, navButton, pageTitle, emitEvent } from "./fixtures";

// What the Rust core tells the main window, through the dev mock's event delivery: a
// `cyberctf://labs/<slug>` link (the deep-link plugin's `deep-link://new-url`, or the one the app
// was opened with: `link=` in the URL), the quit guard (`quit-blocked`), and a website launch onto
// a cloud account waiting for the player's approval (`launch-confirm`). See src/lib/dev-mock.ts.

const DEEP_LINK = "deep-link://new-url";

test.describe("deep links", () => {
  test("a link opened while the app runs shows that lab, whatever screen is open; other links are ignored", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");

    await emitEvent(page, DEEP_LINK, ["cyberctf://labs/goad-light"]);
    await expect(pageTitle(page)).toHaveText("GOAD Light");
    await expect(navButton(page, "Labs")).toHaveAttribute("aria-current", "page");
    // Shown, never started: a web page can't start containers without a click.
    await expect(page.locator("main")).not.toContainText("Running·");

    // Not a lab link, or not one of ours: nothing happens.
    await navButton(page, "Servers").click();
    for (const url of ["https://cyberctf.org/labs/goad-light", "cyberctf://labs/Not_A_Slug!", "cyberctf://account/goad-light"]) {
      await emitEvent(page, DEEP_LINK, [url]);
    }
    await expect(pageTitle(page)).toHaveText("Servers");
    // A lab the catalogue doesn't have: the list.
    await emitEvent(page, DEEP_LINK, ["cyberctf://labs/no-such-lab"]);
    await expect(pageTitle(page)).toHaveText("Labs");
    // The first lab link of a batch wins.
    await emitEvent(page, DEEP_LINK, ["cyberctf://nope", "cyberctf://labs/s3-public-backup", "cyberctf://labs/goad-light"]);
    await expect(pageTitle(page)).toHaveText("Public backup");
  });

  test("the link the app was opened with shows its lab once", async ({ page }) => {
    await openApp(page, `&link=${encodeURIComponent("cyberctf://labs/kube-dashboard-exposed")}`);
    await expect(pageTitle(page)).toHaveText("Exposed dashboard");
    await navButton(page, "Machine").click();
    await expect(pageTitle(page)).toHaveText("Machine");
    await navButton(page, "Labs").click();
    await expect(pageTitle(page)).toHaveText("Labs");
  });
});

test.describe("quit guard", () => {
  test("quitting during a deploy asks first: keep waiting, Escape, or finish in the background", async ({ page }) => {
    await openApp(page);
    const guard = page.getByRole("alertdialog", { name: "A lab is still deploying" });

    await emitEvent(page, "quit-blocked", 2);
    await expect(guard).toBeVisible();
    await expect(guard).toContainText("2 labs are being worked on inside the app.");
    await guard.getByRole("button", { name: "Keep waiting" }).click();
    await expect(guard).toBeHidden();

    await emitEvent(page, "quit-blocked", 1);
    await expect(guard).toContainText("A lab is being worked on inside the app.");
    await page.keyboard.press("Escape");
    await expect(guard).toBeHidden();

    // Finish in background hands the quit to the core (which hides the windows); the choices wait.
    await emitEvent(page, "quit-blocked", 1);
    await guard.getByRole("button", { name: "Finish in background" }).click();
    await expect(guard.getByRole("button", { name: "Finish in background" })).toBeDisabled();
    await expect(guard.getByRole("button", { name: "Quit anyway" })).toBeDisabled();
  });
});

test.describe("cloud launch confirmation", () => {
  test("a website launch onto a cloud account waits for Approve or Decline", async ({ page }) => {
    await openApp(page);
    const request = {
      sessionId: "sess-42",
      repository: "CyberCTF/cloudgoat-ec2-ssrf",
      commit: "0123456789abcdef0123456789abcdef01234567",
      target: "AWS sandbox",
    };
    const confirm = page.getByRole("alertdialog", { name: "Confirm a cloud lab launch" });

    await emitEvent(page, "launch-confirm", request);
    await expect(confirm).toBeVisible();
    await expect(confirm).toContainText("Run this lab on your cloud account?");
    await expect(confirm).toContainText("A launch from the website wants to run on AWS sandbox.");
    await expect(confirm).toContainText("repo: CyberCTF/cloudgoat-ec2-ssrf");
    await expect(confirm).toContainText("commit: 0123456789ab");
    await confirm.getByRole("button", { name: "Run on my cloud" }).click();
    await expect(confirm).toBeHidden();

    // Escape declines.
    await emitEvent(page, "launch-confirm", { ...request, sessionId: "sess-43" });
    await expect(confirm).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(confirm).toBeHidden();
    await emitEvent(page, "launch-confirm", { ...request, sessionId: "sess-44" });
    await confirm.getByRole("button", { name: "Decline" }).click();
    await expect(confirm).toBeHidden();
  });
});
