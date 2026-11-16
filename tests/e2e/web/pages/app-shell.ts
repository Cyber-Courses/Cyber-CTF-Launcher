import type { Locator, Page } from "@playwright/test";

/** A sidebar label as the start of a button's name: the name may go on with a count or a shortcut. */
const startsWith = (label: string) => new RegExp(`^${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`);

/**
 * The main window's frame around every screen (src/components/app-shell.tsx): the sidebar, the
 * title bar over the open screen, the screen itself and the toasts. The specs reach a screen's
 * parts through it, so each part is found one way, here.
 */
export class AppShell {
  constructor(readonly page: Page) {}

  /** The open screen. */
  get main(): Locator {
    return this.page.locator("main");
  }

  /** The open screen's title. */
  get title(): Locator {
    return this.main.getByRole("heading", { level: 1 });
  }

  /** A button of the open screen named exactly `name`. */
  button(name: string): Locator {
    return this.main.getByRole("button", { name, exact: true });
  }

  /** The app's sidebar: the aside that holds the navigation (a lab page has an aside of its own,
   *  its side rail). */
  get sidebar(): Locator {
    return this.page.locator("aside").filter({ has: this.page.locator("nav") });
  }

  /** A sidebar entry by its label. */
  nav(label: string): Locator {
    return this.page.locator("aside nav").getByRole("button", { name: startsWith(label) });
  }

  /** The title bar over the open screen (ShellHeader): the breadcrumb, then the engine and the
   *  hypervisor this machine runs labs on. It has no role or label of its own, so it is found as
   *  the window's drag region at the top of `main`. */
  get header(): Locator {
    return this.page.locator("main > [data-tauri-drag-region]");
  }

  /** The breadcrumb's last crumb (the open screen, or the open lab). */
  get breadcrumb(): Locator {
    return this.header.locator("b");
  }

  /** The toasts' live region (src/components/ui/toaster.tsx), which has no role or label. */
  get toasts(): Locator {
    return this.page.locator("[aria-live=polite]");
  }
}
