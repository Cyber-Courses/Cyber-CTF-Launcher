import { expect, type Locator, type Page } from "@playwright/test";
import { AppShell } from "./app-shell";

/** The Labs screen (src/features/labs/): the catalogue's list, its search and filters, and the
 *  labs imported from a folder. */
export class LabsScreen {
  private readonly app: AppShell;

  constructor(readonly page: Page) {
    this.app = new AppShell(page);
  }

  get search(): Locator {
    return this.page.getByRole("textbox", { name: "Search labs and skills" });
  }

  /** A catalogue lab's row, by its open button ("Open <title>"): the title, a click anywhere on
   *  the row opens the lab. */
  open(title: string): Locator {
    return this.app.button(`Open ${title}`);
  }

  /** Every lab's open button the list shows: one per row. */
  get openButtons(): Locator {
    return this.app.main.getByRole("button", { name: /^Open / });
  }

  /** A lab's title cell, which reads what blocks it (the setup it needs, its failed start) in
   *  place of its description. The row has no role of its own, so it is the open button's parent. */
  titleCell(title: string): Locator {
    return this.open(title).locator("xpath=..");
  }

  /** A lab's whole row: its title cell and its action. */
  row(title: string): Locator {
    return this.titleCell(title).locator("xpath=..");
  }

  /** Goes to the Labs screen from the sidebar. */
  async goTo() {
    await this.app.nav("Labs").click();
  }

  /**
   * Opens the lab titled `title` from the Labs screen and waits for its page (its title, and the
   * breadcrumb's last crumb); with `search`, the search narrows the list to it first.
   */
  async openLab(title: string, { search }: { search?: string } = {}) {
    await this.goTo();
    if (search !== undefined) await this.search.fill(search);
    await this.open(title).click();
    await expect(this.app.title).toHaveText(title);
    await expect(this.app.breadcrumb).toHaveText(title);
  }

  /** Opens the lab imported from a folder titled `title` (its row is "Open <title>, imported from a
   *  folder", apart from a catalogue lab of the same title), and waits for its page. */
  async openImported(title: string) {
    await this.goTo();
    await this.app.button(`Open ${title}, imported from a folder`).click();
    await expect(this.app.title).toHaveText(title);
  }
}
