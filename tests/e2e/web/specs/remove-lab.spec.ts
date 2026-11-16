import { test, expect, openApp, answerConfirm } from "../fixtures";
import type { LabsScreen } from "../pages/labs";
import type { AppShell } from "../pages/app-shell";

// Remove lab, on a lab's page, through the dev mock: an imported lab (`imported` installs one)
// goes once asked, the player lands back on the list, told; a lab that runs can't be removed
// until stopped; a removal that leaves files says which (`remove=partial`). See
// src/lib/dev-mock/scenario.ts.

/** Opens the imported lab from the list's Imported panel. */
async function openImported(app: AppShell, labs: LabsScreen) {
  await labs.goTo();
  await expect(app.main).toContainText("Imported from a folder");
  await labs.openImported("Supplier portal");
}

test.describe("remove lab", () => {
  test("an imported lab is removed once asked, and the list no longer has it", async ({ page, app, labs }) => {
    await openApp(page, "&imported");
    await openImported(app, labs);
    await expect(app.main.getByText("On this computer")).toBeVisible();
    await expect(app.button("Remove lab")).toBeEnabled();

    // Cancel keeps it.
    let asked = answerConfirm(page, false);
    await app.button("Remove lab").click();
    expect(await asked).toContain("Remove Supplier portal from this computer?");
    await expect(app.title).toHaveText("Supplier portal");

    asked = answerConfirm(page, true);
    await app.button("Remove lab").click();
    const question = await asked;
    expect(question).toContain("Cyber CTF deletes its copy of the lab");
    expect(question).toContain("The folder you imported it from is kept as it is.");

    // Back on the list, told; the Imported panel went with its only lab.
    await expect(app.toasts).toContainText("Supplier portal removed");
    await expect(app.toasts).toContainText("The folder you imported it from is untouched.");
    await expect(labs.search).toBeVisible();
    await expect(app.main).not.toContainText("Imported from a folder");
    await expect(app.main.getByRole("button", { name: "Open Supplier portal, imported from a folder" })).toHaveCount(0);
  });

  test("a running lab can't be removed until it is stopped", async ({ page, app, labs }) => {
    await openApp(page, "&deploying=none");
    await app.nav("Labs").click();
    await labs.open("Invoice portal API").click();
    await expect(app.title).toHaveText("Invoice portal API");
    await expect(app.main).toContainText("Running");
    await expect(app.button("Remove lab")).toBeDisabled();
    await app.button("Remove lab").hover({ force: true });
    await expect(app.main.getByRole("tooltip").filter({ hasText: "Stop the lab first" })).toBeVisible();

    // Stopped, it can go.
    const stop = answerConfirm(page, true);
    await app.main.getByRole("button", { name: "Stop & remove" }).click();
    await stop;
    await expect(app.main).toContainText("Not started");
    await expect(app.button("Remove lab")).toBeEnabled();
    const asked = answerConfirm(page, true);
    await app.button("Remove lab").click();
    const question = await asked;
    expect(question).toContain("Remove Invoice portal API from this computer?");
    expect(question).toContain("Your progress stays in your Cyber CTF account");
    await expect(app.toasts).toContainText("Invoice portal API removed");
    await expect(labs.search).toBeVisible();
  });

  test("a removal that leaves files behind says which", async ({ page, app, labs }) => {
    await openApp(page, "&imported&remove=partial");
    await openImported(app, labs);
    // The question, then a second dialog, which stays until read: what stayed, and why.
    const question = answerConfirm(page, true);
    await app.button("Remove lab").click();
    expect(await question).toContain("Remove Supplier portal from this computer?");
    const warning = await answerConfirm(page, true);
    expect(warning).toContain("Supplier portal removed, but some files stayed");
    expect(warning).toContain("runs/cyberctf-local-supplier-portal: Resource busy (os error 16)");
    await expect(labs.search).toBeVisible();
    await expect(app.main).not.toContainText("Imported from a folder");
  });
});
