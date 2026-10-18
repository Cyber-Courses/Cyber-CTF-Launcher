import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, release } from "./fixtures";

// A VM lab (GOAD Light, x86 only) through the dev mock's Vagrant: started on this machine's
// VirtualBox (an Intel Mac) or on a saved ESXi server, paused, shut down, resumed, provisioned
// again on one machine, stopped. `deploying=none`: GOAD Light isn't already deploying at load, as
// it otherwise is in the mock; `hold=provision`: a re-run of provisioning stays in flight until the
// test lets it end (see src/lib/dev-mock/scenario.ts).

const main = (page: Page) => page.locator("main");
const button = (page: Page, name: string) => main(page).getByRole("button", { name, exact: true });
const diagramNode = (page: Page, ip: string) => page.locator(".react-flow .react-flow__node").filter({ hasText: ip });

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

    // This machine is the default (VirtualBox is ready here); the ESXi server is offered too, the
    // AWS account isn't (a VM lab there isn't supported by this one).
    await button(page, "Start lab").click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where.getByRole("radio", { name: /^This machine/ })).toHaveAttribute("aria-checked", "true");
    await expect(where.getByRole("radio", { name: /^office-esxi/ })).toBeEnabled();
    await expect(where.getByRole("radio", { name: /AWS sandbox/ })).toBeDisabled();
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

    await button(page, "Stop & remove").click();
    await page.getByRole("alertdialog", { name: "Remove GOAD Light?" }).getByRole("button", { name: "Remove" }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
    await expect(main(page)).toContainText("Not started");
    await expect(page.locator(".react-flow")).toHaveCount(0);
  });

  // BUG: a VM lab running on this machine has no "Shut down", only Pause. The core reports its
  // place as "local_vm" (runtime/status.rs, local_status: `Runtime::Vm => Place::LocalVm`) and
  // shuts VM labs down fine (lifecycle.rs park: only a *container* lab in a VM refuses it), but
  // lab-state.ts deriveLabState has `canShutdown = … && status?.place !== "local_vm"`, which was
  // meant for a container lab in a VM ("A VM lab gets both", says its comment). So the player can
  // only pause (saved state on disk, GBs per Windows VM) and the remove dialog never offers to shut
  // down instead. Fix (lab-state.ts): `canShutdown = i.parkable && !remote && !isCloud &&
  // !(isDocker && status?.place === "local_vm")`, and the unit test in lab-state.test.ts
  // ("offers pause and shut down by where the lab runs") expecting [true, false] for a VM lab.
  test("on this machine: shut down powers the VMs off, Resume boots them again", async ({ page }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64");
    await openGoad(page);
    await startOn(page, /^This machine/);
    await expect(button(page, "Shut down")).toBeEnabled();

    await button(page, "Shut down").click();
    await expect(button(page, "Resume")).toBeEnabled();
    await expect(main(page)).toContainText("Shut down·VirtualBox");
    await button(page, "Resume").click();
    await expect(button(page, "Shut down")).toBeEnabled();
    await expect(main(page)).toContainText("Running·on this machine");

    // Stop & remove offers to shut down instead.
    await button(page, "Stop & remove").click();
    await expect(page.getByRole("alertdialog", { name: "Remove GOAD Light?" })).toContainText("To keep them, shut the lab down instead.");
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

    await button(page, "Stop & remove").click();
    await page.getByRole("alertdialog", { name: "Remove GOAD Light?" }).getByRole("button", { name: "Remove" }).click();
    await expect(button(page, "Start lab")).toBeEnabled();
  });
});
