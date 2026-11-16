import { expect, type Locator, type Page } from "@playwright/test";
import { AppShell } from "./app-shell";

/** "Where should the lab run?", the dialog Start lab (and Retry) opens: this machine, the saved
 *  servers and cloud accounts, then the form, engine or ports the place needs. */
export class WhereToRun {
  constructor(readonly page: Page) {}

  get dialog(): Locator {
    return this.page.getByRole("dialog", { name: "Where should the lab run?" });
  }

  /** A place or a choice of the dialog, by its radio's name. */
  option(name: string | RegExp): Locator {
    return this.dialog.getByRole("radio", { name });
  }

  /** The row for containers on this machine, under "This machine". */
  get containersHere(): Locator {
    return this.dialog.getByRole("radiogroup", { name: "This machine" }).getByRole("radio", { name: /^Container/ });
  }

  get startButton(): Locator {
    return this.dialog.getByRole("button", { name: "Start", exact: true });
  }

  /** Checks `option` is the one picked. */
  async expectPicked(option: Locator) {
    await expect(option).toHaveAttribute("aria-checked", "true");
  }

  /** Picks `name`, and checks it took. */
  async pick(name: string | RegExp) {
    await this.option(name).click();
    await this.expectPicked(this.option(name));
  }

  /** On a cloud account: Start waits for the player to agree to the lab's own Terraform running
   *  with that account's credentials, then takes it. */
  async agreeToCloudTerraform() {
    await expect(this.startButton).toBeDisabled();
    await this.dialog.getByRole("checkbox", { name: /^I agree that this lab runs its own Terraform code/ }).check();
    await expect(this.startButton).toBeEnabled();
  }

  /** Start, and the dialog closes. */
  async start() {
    await this.startButton.click();
    await expect(this.dialog).toBeHidden();
  }

  /** Cancel, and the dialog closes. */
  async cancel() {
    await this.dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(this.dialog).toBeHidden();
  }
}

/** A lab's page: its header's actions, the failure said under it, the deployment's steps, the
 *  network diagram. */
export class LabScreen {
  private readonly app: AppShell;
  readonly whereToRun: WhereToRun;

  constructor(readonly page: Page) {
    this.app = new AppShell(page);
    this.whereToRun = new WhereToRun(page);
  }

  /** A failure said under the header (as an alert; a toast outside `main` says it too), by its
   *  words, e.g. "Couldn’t start Blind orders". */
  notice(words: string): Locator {
    return this.app.main.getByRole("alert").filter({ hasText: words });
  }

  /** The deployment panel's title. */
  get deployment(): Locator {
    return this.app.main.getByRole("heading", { level: 3, name: /^Deployment/ });
  }

  /** The steps of the deployment (and their machines), one list item each. */
  get steps(): Locator {
    return this.app.main.getByRole("listitem");
  }

  /** The lab's network diagram (React Flow). It has no role or label of its own, so it is found by
   *  React Flow's class. */
  get diagram(): Locator {
    return this.page.locator(".react-flow");
  }

  /** A machine of the diagram, by what its card shows (its address, its name). */
  diagramNode(text: string): Locator {
    return this.diagram.locator(".react-flow__node").filter({ hasText: text });
  }

  /** Presses the header's `from` (Start lab, Retry) and waits for "Where should the lab run?". */
  async askWhere(from: "Start lab" | "Retry" = "Start lab"): Promise<WhereToRun> {
    await this.app.button(from).click();
    await expect(this.whereToRun.dialog).toBeVisible();
    return this.whereToRun;
  }
}
