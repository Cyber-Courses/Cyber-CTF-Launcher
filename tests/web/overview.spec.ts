import { test, expect, openApp } from "./fixtures";

// The Overview's second column and Jump back in, through the dev mock: GOAD Light deploys on the
// saved Proxmox server at load (its step: Configure SRV02), and a lab launched before (its
// `cyberctf.lastrun.<id>` time, src/features/labs/runs/last-run.ts) is offered again.

test.describe("overview", () => {
  test("the deploy panel names the lab deploying, its step and machine, and View opens its page", async ({ page, app }) => {
    await openApp(page);
    const deploy = app.main.getByText("GOAD Light", { exact: true }).first();
    await expect(deploy).toBeVisible();
    await expect(app.main.getByText("Configure SRV02 (ansible 41/88)")).toBeVisible();
    await expect(app.main.getByText("SRV02", { exact: true })).toBeVisible();
    await app.button("View").click();
    await expect(app.title).toHaveText("GOAD Light");
    await expect(app.main).toContainText("Configure SRV02 (ansible 41/88)");
  });

  test("Jump back in lists a lab launched before, how long ago, and opens it", async ({ page, app }) => {
    await page.addInitScript(() => {
      localStorage.setItem("cyberctf.lastrun.lab-kube", String(Date.now() - 2 * 3600_000));
    });
    await openApp(page);
    const jumpBack = app.main.getByRole("heading", { name: "Jump back in" });
    await expect(jumpBack).toBeVisible();
    const open = app.main.getByRole("button", { name: /^Exposed dashboard/ });
    await expect(open).toContainText("kube-dashboard-exposed · last run 2 h ago");
    await expect(app.button("Resume")).toBeEnabled();
    await open.click();
    await expect(app.title).toHaveText("Exposed dashboard");
  });

  test("Jump back in is not shown before any lab was launched here", async ({ page, app }) => {
    await openApp(page);
    await expect(app.main).toContainText("Running now");
    await expect(app.main.getByText("Jump back in")).toHaveCount(0);
  });
});
