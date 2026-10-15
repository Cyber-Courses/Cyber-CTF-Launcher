import type { Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, horizontalOverflow, paletteKey, machineBackAndRetry } from "./fixtures";

// A machine check that fails, the window at its minimum size and larger, and the main flows by
// keyboard alone.

test.describe("a failed machine check", () => {
  test("is said where the report is needed, and Try again recovers", async ({ page }) => {
    // Every check fails (`check=down`, src/lib/dev-mock/scenario.ts) until the machine comes back with the
    // Try again.
    await openApp(page, "&check=down");
    await expect(page.locator("main > [data-tauri-drag-region]")).toContainText("machine check failed");

    await navButton(page, "Machine").click();
    await expect(page.getByText("Couldn’t check this machine")).toBeVisible();
    await expect(page.getByText("docker info timed out after 20 s")).toBeVisible();
    // Labs need the report too: the same message there.
    await navButton(page, "Labs").click();
    await expect(page.getByText("Couldn’t check this machine")).toBeVisible();

    await machineBackAndRetry(page.getByRole("button", { name: "Try again" }));
    await expect(page.locator("main").getByRole("button", { name: "Open Invoice portal API", exact: true })).toBeVisible();
    await expect(page.locator("main > [data-tauri-drag-region]")).not.toContainText("machine check failed");
    await navButton(page, "Machine").click();
    await expect(page.locator("main")).toContainText("Ready for labs");
  });
});

// The window's minimum size (tauri.conf.json: 960x640) and a large one.
for (const size of [
  { width: 960, height: 640 },
  { width: 1600, height: 1000 },
]) {
  test.describe(`window at ${size.width}x${size.height}`, () => {
    test.use({ viewport: size });

    test("every screen fits the width and keeps its primary action in view", async ({ page }) => {
      await openApp(page);
      const main = page.locator("main");
      const screens: [string, string | RegExp, (p: Page) => ReturnType<Page["getByRole"]>][] = [
        ["Overview", /Florian/, (p) => p.locator("main").getByRole("button", { name: "Browse labs" })],
        ["Labs", "Labs", (p) => p.getByRole("textbox", { name: "Search labs and skills" })],
        ["Machine", "Machine", (p) => p.getByRole("radiogroup", { name: "Machine view" })],
        ["Servers", "Servers", (p) => p.getByRole("button", { name: "Add server" })],
        ["Cloud", "Cloud", (p) => p.getByRole("button", { name: "Set up cloud provider" })],
      ];
      for (const [tab, heading, primary] of screens) {
        await navButton(page, tab).click();
        await expect(pageTitle(page)).toHaveText(heading);
        await expect(primary(page)).toBeInViewport();
        expect(await horizontalOverflow(page), `${tab} overflows`).toEqual([]);
      }

      // A lab page: its start button, and the running one's actions.
      await navButton(page, "Labs").click();
      await main.getByRole("button", { name: "Open GOAD Light", exact: true }).click();
      await expect(pageTitle(page)).toHaveText("GOAD Light");
      expect(await horizontalOverflow(page), "GOAD Light overflows").toEqual([]);
      await page.getByRole("button", { name: /All labs/ }).click();
      await main.getByRole("button", { name: "Open Exposed dashboard", exact: true }).click();
      await expect(page.getByRole("button", { name: "Start lab" })).toBeInViewport();
      await page.getByRole("button", { name: "Start lab" }).click();
      const where = page.getByRole("dialog", { name: "Where should the lab run?" });
      // The dialog's actions stay pinned in view, however long its list.
      await expect(where.getByRole("button", { name: "Start", exact: true })).toBeInViewport();
      await expect(where.getByRole("button", { name: "Cancel" })).toBeInViewport();
      await where.getByRole("button", { name: "Cancel" }).click();
      await page.getByRole("button", { name: /All labs/ }).click();
      await main.getByRole("button", { name: "Open Invoice portal API", exact: true }).click();
      await expect(page.locator(".react-flow")).toBeVisible();
      await expect(page.getByRole("button", { name: "Stop & remove" })).toBeInViewport();
      expect(await horizontalOverflow(page), "running lab overflows").toEqual([]);
    });

    test("the onboarding fits too", async ({ page }) => {
      await page.goto("/?mock&onboarding");
      await expect(page.getByRole("button", { name: "Skip setup" })).toBeInViewport();
      await page.getByRole("button", { name: "Get started" }).scrollIntoViewIfNeeded();
      await expect(page.getByRole("button", { name: "Get started" })).toBeInViewport();
      expect(await horizontalOverflow(page), "onboarding overflows").toEqual([]);
    });
  });
}

// Settings and the server setup open in their own windows (window.rs, setup_window.rs): at their
// minimum sizes, nothing scrolls sideways and the way forward is in view.
test("Settings and the setup windows at their minimum sizes", async ({ page }) => {
  await page.setViewportSize({ width: 560, height: 480 });
  await page.goto("/?mock&window=settings");
  for (const section of ["Account", "Appearance", "Labs", "Maintenance", "About"]) {
    await page.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: section }).click();
    await expect(page.getByRole("heading", { level: 2 }).first()).toBeInViewport();
    expect(await horizontalOverflow(page), `Settings ${section} overflows`).toEqual([]);
  }

  await page.setViewportSize({ width: 560, height: 560 });
  await page.goto("/server-setup?mock");
  await expect(page.getByRole("button", { name: "Continue" })).toBeInViewport();
  expect(await horizontalOverflow(page), "server setup overflows").toEqual([]);
  // The cloud list is taller than the window: its Continue is a scroll away.
  await page.goto("/server-setup?mock&kind=cloud");
  await page.getByRole("button", { name: "Continue" }).scrollIntoViewIfNeeded();
  await expect(page.getByRole("button", { name: "Continue" })).toBeInViewport();
  expect(await horizontalOverflow(page), "cloud setup overflows").toEqual([]);
});

/** The focused element's accessible-ish name: its label or text. */
const focused = (page: Page) =>
  page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    return el ? (el.getAttribute("aria-label") ?? el.innerText ?? "").trim() : "";
  });

/** Presses Tab until the focus is on a control whose name matches, at most `max` times. */
async function tabTo(page: Page, name: RegExp, max = 25) {
  for (let n = 0; n < max; n++) {
    await page.keyboard.press("Tab");
    if (name.test(await focused(page))) return;
  }
  throw new Error(`Tab never reached ${name}`);
}

test.describe("keyboard only", () => {
  test("from the sidebar to a lab and back, with Tab, Enter and Escape", async ({ page }) => {
    await openApp(page);
    await tabTo(page, /^Labs/);
    await page.keyboard.press("Enter");
    await expect(pageTitle(page)).toHaveText("Labs");

    await tabTo(page, /^Search labs and skills$/);
    await page.keyboard.type("blind");
    await tabTo(page, /^Open Blind orders$/);
    await page.keyboard.press("Enter");
    await expect(pageTitle(page)).toHaveText("Blind orders");

    await page.keyboard.press("Escape");
    await expect(pageTitle(page)).toHaveText("Labs");
  });

  test("/ jumps to the lab search from anywhere", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Servers").click();
    await page.locator("main h1").click();
    await page.keyboard.press("/");
    await expect(pageTitle(page)).toHaveText("Labs");
    await expect(page.getByRole("textbox", { name: "Search labs and skills" })).toBeFocused();
    // In a text field, "/" is typed.
    await page.keyboard.type("a/b");
    await expect(page.getByRole("textbox", { name: "Search labs and skills" })).toHaveValue("a/b");
  });

  test("the command palette runs the result picked with the arrow keys", async ({ page }) => {
    await openApp(page);
    await page.keyboard.press(await paletteKey(page));
    const palette = page.getByRole("dialog", { name: "Command menu" });
    await expect(palette).toBeVisible();
    await page.keyboard.type("go to");
    const results = palette.getByRole("option");
    const first = (await results.first().innerText()).split("\n")[0];
    const second = (await results.nth(1).innerText()).split("\n")[0];
    expect(first).not.toEqual(second);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(palette).toBeHidden();
    await expect(pageTitle(page)).toHaveText(second.replace(/^Go to /, "").replace("Overview", "") || /Florian/);
  });

  test("the start dialog keeps the focus inside, and Cancel gives it back", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Exposed dashboard", exact: true }).click();
    await page.getByRole("button", { name: "Start lab" }).focus();
    await page.keyboard.press("Enter");
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where).toBeVisible();
    for (let n = 0; n < 8; n++) {
      await page.keyboard.press("Tab");
      expect(await page.evaluate(() => !!document.activeElement?.closest("[role=dialog]")), `Tab ${n + 1} stays in the dialog`).toBe(true);
    }
    await tabTo(page, /^Cancel$/, 8);
    await page.keyboard.press("Enter");
    await expect(where).toBeHidden();
    await expect(page.getByRole("button", { name: "Start lab" })).toBeFocused();
    await expect(pageTitle(page)).toHaveText("Exposed dashboard");
  });

  // BUG: Escape in "Where should it run?" closes the dialog and also leaves the lab page for the
  // list. RunOnDialog (run-on.tsx) closes on a document keydown listener without stopping the
  // event; the dialog is gone by the time the lab page's window listener (useEscapeBack) runs, so
  // that one sees no dialog and goes back. The confirm dialog and the port prompt already stop it.
  // Fix: `e.stopPropagation()` (and preventDefault) in RunOnDialog's Escape handler.
  test("Escape closes the start dialog and stays on the lab page", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Exposed dashboard", exact: true }).click();
    await page.getByRole("button", { name: "Start lab" }).click();
    const where = page.getByRole("dialog", { name: "Where should the lab run?" });
    await expect(where).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(where).toBeHidden();
    await expect(pageTitle(page)).toHaveText("Exposed dashboard");
  });

  // BUG (accessibility): opening a lab from the list with Enter drops the focus on <body>: the row
  // that had it unmounts with the list, and the lab page focuses nothing. The next Tab starts over
  // from the top of the window (the sidebar). Fix: on opening a lab page, focus its heading (with
  // tabIndex={-1}) or the "All labs" back link; do the same when going back to the list.
  test("opening a lab with Enter puts the focus on the lab page", async ({ page }) => {
    await openApp(page);
    await navButton(page, "Labs").click();
    await page.locator("main").getByRole("button", { name: "Open Blind orders", exact: true }).focus();
    await page.keyboard.press("Enter");
    await expect(pageTitle(page)).toHaveText("Blind orders");
    expect(await page.evaluate(() => !!document.activeElement?.closest("main"))).toBe(true);
  });

  // BUG (accessibility): the command palette marks the result Enter will run only visually
  // (`data-active` and a background): no listbox/option roles, no aria-selected, no
  // aria-activedescendant on the search field. A screen reader user moving with the arrow keys
  // hears nothing. Fix (command-palette.tsx): give the list role="listbox", each result
  // role="option" with an id and aria-selected, and point the input's aria-activedescendant at
  // the active one.
  test("the command palette tells assistive tech which result is picked", async ({ page }) => {
    await openApp(page);
    await page.keyboard.press(await paletteKey(page));
    const palette = page.getByRole("dialog", { name: "Command menu" });
    await page.keyboard.press("ArrowDown");
    await expect(palette.getByRole("option", { selected: true })).toHaveCount(1);
  });
});
