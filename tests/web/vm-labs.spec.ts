import { test, expect, openApp, release, answerConfirm } from "./fixtures";
import type { LabsScreen } from "./pages/labs";
import type { LabScreen } from "./pages/lab";
import type { AppShell } from "./pages/app-shell";

// A VM lab (GOAD Light, x86 only) through the dev mock's Vagrant: started on this machine's
// VirtualBox (an Intel Mac) or on a saved ESXi server, paused, shut down, resumed, provisioned
// again on one machine, stopped. On an Apple Silicon Mac (the mock's default machine, UTM ready),
// GOAD Light can only run emulated (QEMU, not installed) or elsewhere, and a VM lab built for arm64
// (Edge router, `catalogue=arm64vm`) runs on UTM. `deploying=none`: GOAD Light isn't already
// deploying at load, as it otherwise is in the mock; `hold=provision`: a re-run of provisioning
// stays in flight until the test lets it end (see src/lib/dev-mock/scenario.ts).

/** "Where should it run?"'s row for this machine: a VM lab's own VMs here, under "This machine". */
const HERE = /^VM On your system/;

/** Opens GOAD Light, not started yet. */
async function openGoad(app: AppShell, labs: LabsScreen) {
  await labs.openLab("GOAD Light", { search: "goad" });
  await expect(app.main).toContainText("Not started");
}

/** Start lab, then Start on the picked place in "Where should it run?". */
async function startOn(lab: LabScreen, place: RegExp) {
  const where = await lab.askWhere();
  await where.pick(place);
  await where.start();
}

test.describe("VM labs", () => {
  test("on this machine: up, pause, resume, provision one machine again, stop", async ({ page, app, lab, labs }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64&hold=provision");
    await openGoad(app, labs);

    // This machine is the default (VirtualBox is ready here); the ESXi server and the AWS account
    // (one cloud VM per machine) are offered too.
    const where = await lab.askWhere();
    await expect(where.option(HERE)).toHaveAttribute("aria-checked", "true");
    await expect(where.option(/^office-esxi/)).toBeEnabled();
    await expect(where.option(/AWS sandbox/)).toBeEnabled();
    await where.start();

    // Vagrant's machines come up one step each.
    const steps = app.main.getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^dc01/ })).toBeVisible();
    await expect(steps.getByRole("button", { name: /^srv02/ })).toBeVisible();
    await expect(app.button("Pause")).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine · VirtualBox");
    for (const ip of ["192.168.56.10", "192.168.56.22"]) await expect(lab.diagramNode(ip)).toBeVisible();
    await expect(page.locator("aside").getByRole("button", { name: /GOAD Light/ })).toBeVisible();

    // Pause: the machines' state saved, Resume brings them back.
    await app.button("Pause").click();
    await expect(app.button("Resume")).toBeEnabled();
    await expect(app.main).toContainText("Paused·VirtualBox");
    await expect(app.button("Pause")).toHaveCount(0);
    await app.button("Resume").click();
    await expect(app.button("Pause")).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine");

    // Provision one machine again, from the Details panel.
    await app.main.getByRole("combobox", { name: "Machine to set up again" }).selectOption("srv02");
    // Pause waits while it runs (held until the test has seen it), then comes back.
    await app.button("Re-run setup").click();
    await expect(app.button("Pause")).toBeDisabled();
    await release(page, "provision");
    await expect(app.button("Pause")).toBeEnabled();
    await app.button("Show full log").click();
    await expect(app.main).toContainText("srv02 : ok=41 changed=3");
    await expect(app.main).toContainText("✓ srv02 provisioned");
    await expect(app.main).not.toContainText("dc01 : ok=41");

    const removed = answerConfirm(page, true);
    await app.button("Stop & remove").click();
    expect(await removed).toContain("Remove GOAD Light?");
    await expect(app.button("Start lab")).toBeEnabled();
    await expect(app.main).toContainText("Not started");
    await expect(lab.diagram).toHaveCount(0);
  });

  // A VM lab here is shut down as well as paused (the backend's controls offer both; only a
  // container lab in one VM can't be), and Stop & remove offers that instead.
  test("on this machine: shut down powers the VMs off, Resume boots them again", async ({ page, app, labs, lab }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64");
    await openGoad(app, labs);
    await startOn(lab, HERE);
    await expect(app.button("Shut down")).toBeEnabled();

    await app.button("Shut down").click();
    await expect(app.button("Resume")).toBeEnabled();
    await expect(app.main).toContainText("Shut down·VirtualBox");
    await app.button("Resume").click();
    await expect(app.button("Shut down")).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine");

    // Stop & remove offers to shut down instead.
    const asked = answerConfirm(page, false);
    await app.button("Stop & remove").click();
    expect(await asked).toContain("To keep them, shut the lab down instead.");
  });

  test("on Apple Silicon: an x86-only lab points at the default server, and this machine needs QEMU", async ({ page, app, lab, labs }) => {
    await openApp(page, "&deploying=none");
    await openGoad(app, labs);
    // Before it starts: where it can run, and that this machine runs it emulated at best.
    await expect(app.main.getByText("Where it runs", { exact: true })).toBeVisible();
    await expect(app.main).toContainText(
      "Built for another CPU: install QEMU from the Machine page to run it emulated (slow), or run it on a server or in your cloud account.",
    );
    await expect(app.main.getByText("VM on this machine", { exact: true }).locator("xpath=..")).toContainText("n/a");

    // No hypervisor here runs it, so Start points at the default server (Proxmox runs GOAD Light);
    // the ESXi server is offered too.
    const where = await lab.askWhere();
    await expect(where.option(/homelab-pve/)).toHaveAttribute("aria-checked", "true");
    await expect(where.option(/office-esxi/)).toBeEnabled();
    // Picked anyway, the backend refuses it before anything is downloaded, saying what it takes.
    await where.option(HERE).click();
    await where.start();
    await expect(app.main).toContainText("Failed to start");
    await expect(app.main.getByRole("alert").filter({ hasText: "Couldn’t start GOAD Light" })).toContainText(
      "No hypervisor ready on this machine runs this lab. Install one from the Machine page, or run the lab on a server or in your cloud account.",
    );
    await expect(lab.diagram).toHaveCount(0);
  });

  test("on Apple Silicon: a VM lab built for arm64 runs on UTM, pauses, resumes and stops", async ({ page, app, labs, lab }) => {
    await openApp(page, "&deploying=none&catalogue=arm64vm");
    await app.nav("Labs").click();
    await labs.search.fill("router");
    await labs.open("Edge router").click();
    await expect(app.title).toHaveText("Edge router");
    await expect(app.main).toContainText("Not started");
    // Native here: no emulation note.
    await expect(app.main).not.toContainText("Built for another CPU");

    // This machine is the default (UTM is ready here).
    await startOn(lab, HERE);
    const steps = app.main.getByRole("listitem");
    await expect(steps.getByRole("button", { name: /^dc01/ })).toBeVisible();
    await expect(app.button("Pause")).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine · UTM");
    await expect(app.main.getByText("Runs on", { exact: true }).locator("xpath=..")).toContainText("this machine · UTM");
    await expect(
      page
        .locator("aside")
        .filter({ has: page.locator("nav") })
        .getByRole("button", { name: /Edge router/ }),
    ).toBeVisible();
    await app.button("Show full log").click();
    await expect(app.main).toContainText("Bringing machine 'dc01' up with 'utm' provider...");

    await app.button("Pause").click();
    await expect(app.button("Resume")).toBeEnabled();
    await expect(app.main).toContainText("Paused·UTM");
    await app.button("Resume").click();
    await expect(app.button("Pause")).toBeEnabled();
    await expect(app.main).toContainText("Running·on this machine · UTM");

    const removed = answerConfirm(page, true);
    await app.button("Stop & remove").click();
    expect(await removed).toContain("Remove Edge router?");
    await expect(app.button("Start lab")).toBeEnabled();
    await expect(app.main).toContainText("Not started");
  });

  test("on a saved ESXi server: runs there, provisioning again works, nothing to pause, then stop", async ({ page, app, labs, lab }) => {
    await openApp(page, "&deploying=none&os=macos&arch=x86_64");
    await openGoad(app, labs);
    await startOn(lab, /^office-esxi/);

    await expect(app.button("Stop & remove")).toBeEnabled();
    await expect(app.main).toContainText("Running·on office-esxi");
    // Remote VMs can't be parked from here, and their attack box isn't this machine's.
    await expect(app.button("Pause")).toHaveCount(0);
    await expect(app.button("Shut down")).toHaveCount(0);
    await expect(app.button("Re-run setup")).toBeEnabled();
    await expect(app.main.getByText("Runs on")).toBeVisible();

    const removed = answerConfirm(page, true);
    await app.button("Stop & remove").click();
    expect(await removed).toContain("Remove GOAD Light?");
    await expect(app.button("Start lab")).toBeEnabled();
  });
});
