import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

// The attack box shell window (`/shell`, xterm.js), opened from a running lab's page and answered
// by the dev mock's stand-in shell: it echoes, answers `id`, `whoami`, `hostname` and `ip -br a`,
// and ends on `exit` (see src/lib/dev-mock/terminal.ts).

/** Types a command line in the terminal and runs it. */
async function run(shell: Page, line: string) {
  await shell.keyboard.type(line);
  await shell.keyboard.press("Enter");
}

test.describe("attack box shell", () => {
  test("type commands, see the answers, exit, reconnect, close", async ({ page, context }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");

    const [shell] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Open attacker shell" }).click()]);
    await expect(shell).toHaveURL(/\/shell\?id=lab-invoice&runtime=DOCKER&mock$/);
    await expect(shell.getByText("attack box", { exact: true })).toBeVisible();
    const terminal = shell.locator(".xterm-rows");
    await expect(terminal).toContainText("Connected to the attack box (dev mock).");
    await shell.locator(".xterm").click();

    await run(shell, "whoami");
    await expect(terminal).toContainText(/root@attack-box:~# whoami\s*root/);
    await run(shell, "hostname");
    await expect(terminal).toContainText(/hostname\s*attack-box/);
    await run(shell, "ip -br a");
    await expect(terminal).toContainText("10.20.0.2/24");
    // Backspace edits the line before it runs.
    await shell.keyboard.type("idd");
    await shell.keyboard.press("Backspace");
    await shell.keyboard.press("Enter");
    await expect(terminal).toContainText("uid=0(root) gid=0(root) groups=0(root)");
    await run(shell, "nmap");
    await expect(terminal).toContainText("nmap: command not found (dev mock)");

    // The shell ends: said, with Reconnect and Close.
    await run(shell, "exit");
    await expect(shell.getByText("The shell ended.")).toBeVisible();
    await expect(shell.getByText("session ended", { exact: true })).toBeVisible();
    await shell.getByRole("button", { name: "Reconnect" }).click();
    await expect(shell.getByText("attack box", { exact: true })).toBeVisible();
    await expect(shell.getByText("The shell ended.")).toHaveCount(0);
    await shell.locator(".xterm").click();
    await run(shell, "id");
    await expect(terminal).toContainText("uid=0(root)");

    await run(shell, "exit");
    const closed = shell.waitForEvent("close");
    await shell.getByRole("button", { name: "Close", exact: true }).click();
    await closed;
  });
});
