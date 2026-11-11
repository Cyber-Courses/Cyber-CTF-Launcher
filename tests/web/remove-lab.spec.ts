import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, answerConfirm } from "./fixtures";

// Remove lab, on a lab's page, through the dev mock: an imported lab (`imported` installs one)
// goes once asked, the player lands back on the list, told; a lab that runs can't be removed
// until stopped; a removal that leaves files says which (`remove=partial`). See
// src/lib/dev-mock/scenario.ts.

const main = (page: Page) => page.locator("main");
const removeButton = (page: Page) => main(page).getByRole("button", { name: "Remove lab", exact: true });
const toasts = (page: Page) => page.locator("[aria-live=polite]");

/** Opens the imported lab from the list's Imported panel. */
async function openImported(page: Page) {
  await navButton(page, "Labs").click();
  await expect(main(page)).toContainText("Imported from a folder");
  await main(page).getByRole("button", { name: "Open Supplier portal", exact: true }).click();
  await expect(pageTitle(page)).toHaveText("Supplier portal");
}

test.describe("remove lab", () => {
  test("an imported lab is removed once asked, and the list no longer has it", async ({ page }) => {
    await openApp(page, "&imported");
    await openImported(page);
    await expect(main(page).getByText("On this computer")).toBeVisible();
    await expect(removeButton(page)).toBeEnabled();

    // Cancel keeps it.
    let asked = answerConfirm(page, false);
    await removeButton(page).click();
    expect(await asked).toContain("Remove Supplier portal from this computer?");
    await expect(pageTitle(page)).toHaveText("Supplier portal");

    asked = answerConfirm(page, true);
    await removeButton(page).click();
    const question = await asked;
    expect(question).toContain("Cyber CTF deletes its copy of the lab");
    expect(question).toContain("The folder you imported it from is kept as it is.");

    // Back on the list, told; the Imported panel went with its only lab.
    await expect(toasts(page)).toContainText("Supplier portal removed");
    await expect(toasts(page)).toContainText("The folder you imported it from is untouched.");
    await expect(main(page).getByRole("textbox", { name: "Search labs and skills" })).toBeVisible();
    await expect(main(page)).not.toContainText("Imported from a folder");
    await expect(main(page).getByRole("button", { name: "Open Supplier portal" })).toHaveCount(0);
  });

  test("a running lab can't be removed until it is stopped", async ({ page }) => {
    await openApp(page, "&deploying=none");
    await navButton(page, "Labs").click();
    await main(page).getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
    await expect(pageTitle(page)).toHaveText("Invoice portal API");
    await expect(main(page)).toContainText("Running");
    await expect(removeButton(page)).toBeDisabled();
    await removeButton(page).hover({ force: true });
    await expect(main(page).getByRole("tooltip").filter({ hasText: "Stop the lab first" })).toBeVisible();

    // Stopped, it can go.
    const stop = answerConfirm(page, true);
    await main(page).getByRole("button", { name: "Stop & remove" }).click();
    await stop;
    await expect(main(page)).toContainText("Not started");
    await expect(removeButton(page)).toBeEnabled();
    const asked = answerConfirm(page, true);
    await removeButton(page).click();
    const question = await asked;
    expect(question).toContain("Remove Invoice portal API from this computer?");
    expect(question).toContain("Your progress stays in your Cyber CTF account");
    await expect(toasts(page)).toContainText("Invoice portal API removed");
    await expect(main(page).getByRole("textbox", { name: "Search labs and skills" })).toBeVisible();
  });

  test("a removal that leaves files behind says which", async ({ page }) => {
    await openApp(page, "&imported&remove=partial");
    await openImported(page);
    // The question, then a second dialog, which stays until read: what stayed, and why.
    const said: string[] = [];
    page.on("dialog", (dialog) => {
      said.push(dialog.message());
      void dialog.accept();
    });
    await removeButton(page).click();
    await expect.poll(() => said.length).toBe(2);
    expect(said[0]).toContain("Remove Supplier portal from this computer?");
    expect(said[1]).toContain("Supplier portal removed, but some files stayed");
    expect(said[1]).toContain("runs/cyberctf-local-supplier-portal: Resource busy (os error 16)");
    await expect(main(page).getByRole("textbox", { name: "Search labs and skills" })).toBeVisible();
    await expect(main(page)).not.toContainText("Imported from a folder");
  });
});
