import { expect, type Locator, type Page } from "@playwright/test";

/** The Settings window (src/features/settings/): its sections' navigation and the open section. */
export class SettingsWindow {
  constructor(readonly page: Page) {}

  get sections(): Locator {
    return this.page.getByRole("navigation", { name: "Settings sections" });
  }

  /** A section's tab in the navigation. */
  tab(section: string): Locator {
    return this.sections.getByRole("button", { name: section });
  }

  /** Opens the Settings window in this page, as the app opens it (`open_settings`), on `section`. */
  async open(section: string) {
    await this.page.goto("/?mock&window=settings");
    await this.show(section);
  }

  /** Shows `section`, and checks the navigation says it is the one open. */
  async show(section: string) {
    await this.tab(section).click();
    await expect(this.tab(section)).toHaveAttribute("aria-current", "page");
  }
}
