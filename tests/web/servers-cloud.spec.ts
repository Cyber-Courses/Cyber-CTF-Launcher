import { test, expect, openApp, navButton, pageTitle } from "./fixtures";

test.describe("servers and cloud", () => {
  test.beforeEach(async ({ page }) => openApp(page));

  test("Servers lists the hosts", async ({ page }) => {
    await navButton(page, "Servers").click();
    await expect(pageTitle(page)).toHaveText("Servers");
    const main = page.locator("main");
    await expect(main.getByText("homelab-pve", { exact: true })).toBeVisible();
    await expect(main.getByText("office-esxi", { exact: true })).toBeVisible();
    await expect(main).toContainText("192.168.1.20");
    // Both hosts answer the mock's test.
    await expect(main.getByText("2 of 2 online", { exact: false })).toBeVisible();
    // Cloud accounts live on the Cloud screen, not here.
    await expect(main.getByText("AWS sandbox")).toHaveCount(0);
  });

  test("Add server opens the setup", async ({ page, context }) => {
    await navButton(page, "Servers").click();
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Add server" }).click()]);
    await expect(setup).toHaveURL(/\/server-setup\?mock$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Connect a host" })).toBeVisible();
    await expect(setup.getByText(/^Step 1 of \d+$/)).toBeVisible();
  });

  test("Cloud lists the accounts", async ({ page }) => {
    await navButton(page, "Cloud").click();
    await expect(pageTitle(page)).toHaveText("Cloud");
    const main = page.locator("main");
    await expect(main.getByText("AWS sandbox", { exact: true })).toBeVisible();
    await expect(main).toContainText("eu-west-3");
    await expect(main.getByText("Azure", { exact: true })).toBeVisible();
    await expect(main.getByText("Google Cloud", { exact: true })).toBeVisible();
  });

  test("Set up cloud provider opens the cloud setup", async ({ page, context }) => {
    await navButton(page, "Cloud").click();
    const [setup] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Set up cloud provider" }).click()]);
    await expect(setup).toHaveURL(/\/server-setup\?kind=cloud&mock$/);
    await expect(setup.getByRole("heading", { level: 1, name: "Set up cloud provider" })).toBeVisible();
  });
});

// Each page's empty state, through the shared EmptyState: a region named by its h3, the providers
// it takes as a group of avatars (named for screen readers and in tooltips, no text; past eight a
// "+N"), and the one action that fills the page (the header has no copy of it then). `servers=none`,
// `cloud=none`: no saved server or cloud account; `machine=fresh`: no engine or hypervisor installed
// (on the mock's Apple Silicon Mac).
test.describe("empty states", () => {
  const pages = [
    { nav: "Servers", query: "&servers=none", title: "No server connected", action: "Add server", avatars: ["Proxmox", "VMware ESXi"] },
    {
      nav: "Cloud",
      query: "&cloud=none",
      title: "No cloud account connected",
      action: "Set up cloud provider",
      avatars: ["AWS", "Azure", "Google Cloud", "DigitalOcean", "Linode", "Oracle Cloud"],
    },
    {
      nav: "Machine",
      query: "&machine=fresh",
      title: "No provider installed",
      action: "Add a provider",
      avatars: ["Docker Desktop", "OrbStack", "Colima", "VirtualBox", "VMware Fusion", "UTM", "Parallels"],
    },
  ];
  for (const p of pages) {
    test(`${p.nav} shows its heading, its providers' avatars and its one action`, async ({ page }) => {
      await openApp(page, p.query);
      await navButton(page, p.nav).click();
      await expect(pageTitle(page)).toHaveText(p.nav);
      const empty = page.locator("main").getByRole("region", { name: p.title });
      await expect(empty.getByRole("heading", { level: 3, name: p.title })).toBeVisible();
      await expect(empty.getByRole("img")).toHaveCount(p.avatars.length);
      for (const [n, name] of p.avatars.entries()) await expect(empty.getByRole("img").nth(n)).toHaveAccessibleName(name);
      await expect(empty.getByRole("button", { name: p.action })).toBeVisible();
      await expect(page.locator("main").getByRole("button", { name: p.action })).toHaveCount(1);
    });
  }

  test("an avatar names its provider in a tooltip on hover; Cloud's are no buttons and it has no Connect row", async ({ page }) => {
    await openApp(page, "&cloud=none");
    await navButton(page, "Cloud").click();
    await expect(page.locator("main").getByRole("region", { name: "Connect a cloud provider" })).toHaveCount(0);
    const empty = page.locator("main").getByRole("region", { name: "No cloud account connected" });
    await expect(empty.getByRole("button")).toHaveCount(1);
    const azure = empty.getByRole("img", { name: "Azure" });
    const tip = azure.locator("xpath=following-sibling::*[@role='tooltip']");
    await expect(tip).toHaveCSS("opacity", "0");
    await azure.hover();
    await expect(tip).toHaveCSS("opacity", "1");
    await expect(tip).toHaveText("Azure");
  });

  test("ESXi, VMware Fusion and Parallels show their initials", async ({ page }) => {
    await openApp(page, "&servers=none&machine=fresh");
    await navButton(page, "Servers").click();
    await expect(page.locator("main").getByRole("img", { name: "VMware ESXi" })).toHaveText("ES");
    await navButton(page, "Machine").click();
    const machine = page.locator("main").getByRole("region", { name: "No provider installed" });
    await expect(machine.getByRole("img", { name: "VMware Fusion" })).toHaveText("VF");
    await expect(machine.getByRole("img", { name: "Parallels" })).toHaveText("PD");
  });

  test("Machine's empty state offers a server instead", async ({ page }) => {
    await openApp(page, "&machine=fresh");
    await navButton(page, "Machine").click();
    await page.locator("main").getByRole("region", { name: "No provider installed" }).getByRole("button", { name: "Use a server" }).click();
    await expect(pageTitle(page)).toHaveText("Servers");
  });
});
