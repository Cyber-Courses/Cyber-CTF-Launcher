import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, release, answerConfirm } from "./fixtures";

// A VM lab (GOAD Light, x86 only) through the dev mock's Vagrant: started on this machine's
// VirtualBox (an Intel Mac) or on a saved ESXi server, paused, shut down, resumed, provisioned
// again on one machine, stopped. On an Apple Silicon Mac (the mock's default machine, UTM ready),
// GOAD Light can only run emulated (QEMU, not installed) or elsewhere, and a VM lab built for arm64
// (Edge router, `catalogue=arm64vm`) runs on UTM. `deploying=none`: GOAD Light isn't already
// deploying at load, as it otherwise is in the mock; `hold=provision`: a re-run of provisioning
// stays in flight until the test lets it end (see src/lib/dev-mock/scenario.ts).

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });
const diagramNode = (page: Page, ip: string) => page.locator(".react-flow .react-flow__node").filter({ hasText: ip });
/** "Where should it run?"'s row for this machine: a VM lab's own VMs here, under "This machine". */
const HERE = /^VM On your system/;

async function openGoad(page: Page) {
  await navButton(page, "Labs").click();
  await page.getByRole("textbox", { name: "Search labs and skills" }).fill("goad");
  await main(page).getByRole("button", { name: "Open GOAD Light", exact: true }).click();
  await expect(pageTitle(page)).toHaveText("GOAD Light");
  await expect(main(page)).toContainText("Not started");
}

/** Start lab, then Start on the picked place in "Where should it run?". */
async function startOn(page: Page, place: RegExp) {
  await button(page, "Start lab").click();
  const where = page.getByRole("dialog", { name: "Where should the lab run?" });
  await where.getByRole("radio", { name: place }).click();
  await expect(where.getByRole("radio", { name: place })).toHaveAttribute("aria-checked", "true");
  await where.getByRole("button", { name: "Start", exact: true }).click();
  await expect(where).toBeHidden();
}

test.describe("VM labs", () => {
  test("on this machine: up, pause, resume, provision one machine again, stop", async ({ page }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64&hold=provision");
    await openGoad(page);

    // This machine is the default (VirtualBox is ready here); the ESXi server and the AWS account
    // (one cloud VM per machine) are offered too.
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: HERE })).toHaveAttribute("aria-checked", "true");
    await expect(where.getByRole("radio", { name: /^office-esxi/ })).toBeEnabled();
    await expect(where.getByRole("radio", { name: /AWS sandbox/ })).toBeEnabled();
    await where.getByRole("button", { name: "Start", exact: true }).click();

    // Vagrant's machines come up one step each.
    const steps = main(page).getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^dc01/ })).toBeVisible();
    await expect(steps.getByRole("button", { name: /^srv02/ })).toBeVisible();
    await expect(button(page, "Pause")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine · VirtualBox");
    for (const ip of ["192.168.56.10", "192.168.56.22"]) await expect(diagramNode(page, ip)).toBeVisible();
    await expect(page.locator("aside").getByRole("button", { name: /GOAD Light/ })).toBeVisible();

    // Pause: the machines' state saved, Resume brings them back.
    await button(page, "Pause").click();
    await expect(button(page, "Resume")).toBeEnabled();
    await expect(main(page)).toContainText("Paused·VirtualBox");
    await expect(button(page, "Pause")).toHaveCount(0);
    await button(page, "Resume").click();
    await expect(button(page, "Pause")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine");

    // Provision one machine again, from the Details panel.
    await main(page).getByRole("combobox", { name: "Machine to set up again" }).selectOption("srv02");
    // Pause waits while it runs (held until the test has seen it), then comes back.
    await button(page, "Re-run setup").click();
    await expect(button(page, "Pause")).toBeDisabled();
    await release(page, "provision");
    await expect(button(page, "Pause")).toBeEnabled();
    await button(page, "Show full log").click();
    await expect(main(page)).toContainText("srv02 : ok=41 changed=3");
    await expect(main(page)).toContainText("✓ srv02 provisioned");
    await expect(main(page)).not.toContainText("dc01 : ok=41");

    const removed = answerConfirm(page, true);
    await button(page, "Stop & remove").click();
    expect(await removed).toContain("Remove GOAD Light?");
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(main(page)).toContainText("Not started");
    await expect(page.locator(".react-flow")).toHaveCount(0);
  });

  // A VM lab here is shut down as well as paused (the backend's controls offer both; only a
  // container lab in one VM can't be), and Stop & remove offers that instead.
  test("on this machine: shut down powers the VMs off, Resume boots them again", async ({ page }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64");
    await openGoad(page);
    await startOn(page, HERE);
    await expect(button(page, "Shut down")).toBeEnabled();

    await button(page, "Shut down").click();
    await expect(button(page, "Resume")).toBeEnabled();
    await expect(main(page)).toContainText("Shut down·VirtualBox");
    await button(page, "Resume").click();
    await expect(button(page, "Shut down")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine");

    // Stop & remove offers to shut down instead.
    const asked = answerConfirm(page, false);
    await button(page, "Stop & remove").click();
    expect(await asked).toContain("To keep them, shut the lab down instead.");
  });

  test("on Apple Silicon: an x86-only lab points at the default server, and this machine needs QEMU", async ({ page }) => {
    await openApp(page, "&deploying=none");
    await openGoad(page);
    // Before it starts: where it can run, and that this machine runs it emulated at best.
    await expect(main(page).getByText("Where it runs", { exact: true })).toBeVisible();
    await expect(main(page)).toContainText(
      "Built for another CPU: install QEMU from the Machine page to run it emulated (slow), or run it on a server or in your cloud account.",
    );
    await expect(main(page).getByText("VM on this machine", { exact: true }).locator("xpath=..")).toContainText("n/a");

    // No hypervisor here runs it, so Start points at the default server (Proxmox runs GOAD Light);
    // the ESXi server is offered too.
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: /homelab-pve/ })).toHaveAttribute("aria-checked", "true");
    await expect(where.getByRole("radio", { name: /office-esxi/ })).toBeEnabled();
    // Picked anyway, the backend refuses it before anything is downloaded, saying what it takes.
    await where.getByRole("radio", { name: HERE }).click();
    await where.getByRole("button", { name: "Start", exact: true }).click();
    await expect(where).toBeHidden();
    await expect(main(page)).toContainText("Failed to start");
    await expect(main(page).getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" })).toContainText(
      "No hypervisor ready on this machine runs this lab. Install one from the Machine page, or run the lab on a server or in your cloud account.",
    );
    await expect(page.locator(".react-flow")).toHaveCount(0);
  });

  test("on Apple Silicon: a VM lab built for arm64 runs on UTM, pauses, resumes and stops", async ({ page }) => {
    await openApp(page, "&deploying=none&catalogue=arm64vm");
    await navButton(page, "Labs").click();
    await page.getByRole("textbox", { name: "Search labs and skills" }).fill("router");
    await main(page).getByRole("button", { name: "Open Edge router", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Edge router");
    await expect(main(page)).toContainText("Not started");
    // Native here: no emulation note.
    await expect(main(page)).not.toContainText("Built for another CPU");

    // This machine is the default (UTM is ready here).
    await startOn(page, HERE);
    const steps = main(page).getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^dc01/ })).toBeVisible();
    await expect(button(page, "Pause")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine · UTM");
    await expect(main(page).getByText("Runs on", { exact: true }).locator("xpath=..")).toContainText("this machine · UTM");
    await expect(
      page
        .locator("aside")
        .filter({ has: page.locator("nav") })
        .getByRole("button", { name: /Edge router/ }),
    ).toBeVisible();
    await button(page, "Show full log").click();
    await expect(main(page)).toContainText("Bringing machine 'dc01' up with 'utm' provider...");

    await button(page, "Pause").click();
    await expect(button(page, "Resume")).toBeEnabled();
    await expect(main(page)).toContainText("Paused·UTM");
    await button(page, "Resume").click();
    await expect(button(page, "Pause")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine · UTM");

    const removed = answerConfirm(page, true);
    await button(page, "Stop & remove").click();
    expect(await removed).toContain("Remove Edge router?");
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(main(page)).toContainText("Not started");
  });

  test("on a saved ESXi server: runs there, provisioning again works, nothing to pause, then stop", async ({ page }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64");
    await openGoad(page);
    await startOn(page, /^office-esxi/);

    await expect(button(page, "Stop & remove")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on office-esxi");
    // Remote VMs can't be parked from here, and their attack box isn't this machine's.
    await expect(button(page, "Pause")).toHaveCount(0);
    await expect(button(page, "Shut down")).toHaveCount(0);
    await expect(button(page, "Re-run setup")).toBeEnabled();
    await expect(main(page).getByText("Runs on")).toBeVisible();

    const removed = answerConfirm(page, true);
    await button(page, "Stop & remove").click();
    expect(await removed).toContain("Remove GOAD Light?");
    await expect(button(page, "Start lab")).toBeEnabled();
  });
});
