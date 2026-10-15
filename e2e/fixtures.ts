import { test as base, expect, type Locator, type Page } from "@playwright/test";

/** The six languages the launcher ships (src/lib/i18n). */
export const LOCALES = ["en", "fr", "es", "de", "pt-BR", "ja"] as const;
export type Locale = (typeof LOCALES)[number];

/** What a missing translation renders: the key itself, like "labs.list.search". */
export const RAW_KEY = /^[a-z]+(\.[a-zA-Z]+)+$/;

type Fixtures = {
  /** The language the test starts in (localStorage `cyberctf.locale`, unless a page set one). */
  appLocale: Locale;
  /** Console errors, uncaught exceptions and missing-translation warnings, on every page of the
   *  test; the test fails if any were seen. */
  consoleErrors: string[];
};

export const test = base.extend<Fixtures>({
  appLocale: ["en", { option: true }],
  consoleErrors: [
    async ({ context, appLocale }, use) => {
      const errors: string[] = [];
      // Seeded before any app script runs, on every page (Settings and the server setup open in
      // new tabs): the language, and onboarding done so the shell shows.
      await context.addInitScript((locale) => {
        try {
          // Only when not set yet: a language picked during the test survives a reload.
          if (!localStorage.getItem("cyberctf.locale")) localStorage.setItem("cyberctf.locale", locale);
          localStorage.setItem("cyberctf.onboarded", "1");
        } catch {
          /* storage unavailable */
        }
      }, appLocale);
      const watch = (page: Page) => {
        page.on("console", (m) => {
          const text = m.text();
          if (m.type() === "error" || (m.type() === "warning" && text.startsWith("i18n: missing message"))) errors.push(`${page.url()}: ${text}`);
        });
        page.on("pageerror", (e) => errors.push(`${page.url()}: ${e.message}`));
      };
      context.pages().forEach(watch);
      context.on("page", watch);
      await use(errors);
      expect(errors, "console errors").toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };

/** Opens the main window with the mock and waits for the shell. `query` adds the mock's
 *  scenario switches (src/lib/dev-mock.ts, `Scenario`), e.g. "&launch=fail". */
export async function openApp(page: Page, query = "") {
  await page.goto(`/?mock${query}`);
  await expect(page.locator("aside nav")).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
}

/** With `check=down` (src/lib/dev-mock.ts): brings the machine back and presses `retry` in one
 *  go, inside the page, so no background re-check can answer in between. */
export async function machineBackAndRetry(retry: Locator) {
  await retry.evaluate((button: HTMLElement) => {
    (window as unknown as { __CYBERCTF_MOCK_MACHINE_UP__: () => void }).__CYBERCTF_MOCK_MACHINE_UP__();
    button.click();
  });
}

/** The command palette's shortcut where the page thinks it runs: ⌘K on macOS, Ctrl+K elsewhere
 *  (by the user agent, which the "Desktop Chrome" device sets to Windows). */
export async function paletteKey(page: Page) {
  return (await page.evaluate(() => navigator.userAgent)).includes("Mac") ? "Meta+k" : "Control+k";
}

/** A sidebar entry by its label (its name may go on with a count or a shortcut). */
export const navButton = (page: Page, name: string) =>
  page.locator("aside nav").getByRole("button", { name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`) });

/** The open screen's title. */
export const pageTitle = (page: Page) => page.locator("main").getByRole("heading", { level: 1 });

/** The header breadcrumb's last crumb (the open screen, or the open lab). */
export const breadcrumb = (page: Page) => page.locator("main > [data-tauri-drag-region] b");

/** Visible texts (and placeholders, labels) on the page that look like a raw message key. */
export function rawKeys(page: Page): Promise<string[]> {
  return page.evaluate((source) => {
    const re = new RegExp(source);
    const found = new Set<string>();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const text = n.textContent?.trim() ?? "";
      const el = n.parentElement;
      if (text && re.test(text) && el && el.checkVisibility()) found.add(text);
    }
    for (const el of document.querySelectorAll<HTMLElement>("[placeholder], [aria-label], [title]")) {
      for (const attr of ["placeholder", "aria-label", "title"]) {
        const v = el.getAttribute(attr)?.trim();
        if (v && re.test(v)) found.add(`${attr}=${v}`);
      }
    }
    return [...found];
  }, RAW_KEY.source);
}

/** Whether the page or its main scroll area scrolls sideways: [scrollWidth, clientWidth] pairs. */
export function horizontalOverflow(page: Page) {
  return page.evaluate(() => {
    const boxes = [document.documentElement, document.body, ...document.querySelectorAll<HTMLElement>("main .overflow-y-auto")];
    return boxes.filter((b) => b.scrollWidth > b.clientWidth + 1).map((b) => `${b.tagName.toLowerCase()}.${b.className}: ${b.scrollWidth} > ${b.clientWidth}`);
  });
}

/** How a forced command answers: an error (after `delay` ms), or a value. */
export type Forced = { error: string; delay?: number } | { value: unknown; delay?: number };

/**
 * Makes some Tauri commands fail (or answer `value`) on every page of the test, ahead of the dev
 * mock, which answers the rest: the mock installs its `invoke` on `__TAURI_INTERNALS__`, and this
 * wraps whatever is installed. Call before `openApp`.
 */
export async function forceCommands(page: Page, forced: Record<string, Forced>) {
  await page.addInitScript((forced: Record<string, Forced>) => {
    type Invoke = (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
    let real: Invoke | null = null;
    // Callbacks and window metadata, for what registers before the mock is installed (event
    // listeners on mount); the mock replaces them with its own.
    const callbacks = new Map<number, unknown>();
    let next = 1;
    const internals = {
      transformCallback: (cb: unknown) => {
        callbacks.set(next, cb);
        return next++;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    } as { invoke?: Invoke };
    Object.defineProperty(internals, "invoke", {
      configurable: true,
      enumerable: true,
      set(fn: Invoke) {
        real = fn;
      },
      get() {
        return async (cmd: string, args?: unknown, options?: unknown) => {
          const f = forced[cmd];
          if (!f) return real?.(cmd, args, options);
          if (f.delay) await new Promise((r) => setTimeout(r, f.delay));
          if ("error" in f) throw f.error;
          return structuredClone(f.value);
        };
      },
    });
    const w = window as unknown as { __TAURI_INTERNALS__: unknown; __CYBERCTF_MOCK__: boolean };
    w.__TAURI_INTERNALS__ = internals;
    // Lets the dev mock install itself over these internals.
    w.__CYBERCTF_MOCK__ = true;
  }, forced);
}

/** Sends an app event (what the Rust core emits: `quit-blocked`, `deep-link://new-url`…) to the
 *  windows of the test's context, through the dev mock's event bus (src/lib/dev-mock.ts). */
export function emitEvent(page: Page, event: string, payload: unknown) {
  return page.evaluate(
    ([event, payload]) => {
      const bus = new BroadcastChannel("cyberctf-dev-mock-events");
      bus.postMessage({ event, payload });
      bus.close();
    },
    [event, payload] as const,
  );
}
