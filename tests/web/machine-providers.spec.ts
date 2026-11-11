import type { Locator, Page } from "@playwright/test";
import { test, expect, openApp, navButton, pageTitle, animationsFinished, horizontalOverflow, type Locale } from "./fixtures";
import en from "../../src/messages/en/machine.json";
import de from "../../src/messages/de/machine.json";
import fr from "../../src/messages/fr/machine.json";

// The Machine page's providers as a table: every row's status, default slot and action sit in
// the same columns, and making another provider the default (or opening a row's test) moves no
// name and no status. `machine=many` (src/lib/dev-mock/scenario.ts): Docker Desktop (labs' engine),
// OrbStack running, Colima stopped; UTM (labs' hypervisor), Parallels ready, VMware Fusion short
// of its plugin.

type Box = { x: number; y: number; width: number; height: number };
type Strings = typeof en;

const providers = (page: Page) => page.locator("main").getByRole("list", { name: /./ }).first();
// Its own rows only: an open self-test lists its steps inside one.
const rows = (page: Page) => providers(page).locator(':scope > [role="listitem"]');
const row = (page: Page, name: string) => rows(page).filter({ has: page.locator('[data-slot="name"]', { hasText: name }) });
const slot = (r: Locator, name: string) => r.locator(`[data-slot="${name}"]`).first();

/** Where each row's name and status are, by provider. */
async function layout(page: Page): Promise<Record<string, { name: Box; status: Box }>> {
  return rows(page).evaluateAll((items) =>
    Object.fromEntries(
      items.map((item) => {
        const rect = (sel: string) => {
          const r = item.querySelector(sel)?.getBoundingClientRect();
          return r
            ? { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) }
            : { x: -1, y: -1, width: 0, height: 0 };
        };
        return [item.querySelector('[data-slot="name"]')?.textContent ?? "", { name: rect('[data-slot="name"]'), status: rect('[data-slot="status"]') }];
      }),
    ),
  );
}

/** The left edge of every row's cell `name` (status, default slot, actions). */
const columnXs = (page: Page, name: string) =>
  rows(page).evaluateAll((items, n) => items.map((i) => Math.round(i.querySelector(`[data-slot="${n}"]`)?.getBoundingClientRect().x ?? -1)), name);

async function openMachine(page: Page, s: Strings) {
  await openApp(page, "&machine=many&deploying=none");
  await navButton(page, s.screen.title).click();
  await expect(pageTitle(page)).toHaveText(s.screen.title);
  await expect(rows(page)).toHaveCount(6);
  await animationsFinished(page);
}

/** Every column at one x in every row, each cell on one line, and nothing wider than the window. */
async function expectAligned(page: Page) {
  for (const column of ["status", "default", "actions"]) expect(new Set(await columnXs(page, column)), `${column} column`).toHaveProperty("size", 1);
  const tall = await rows(page).evaluateAll((items) =>
    items.flatMap((i) =>
      ['[data-slot="status"]', '[data-slot="default"] > *', '[data-slot="actions"] > *']
        .flatMap((sel) => Array.from(i.querySelectorAll<HTMLElement>(sel)))
        .filter((el) => el.getBoundingClientRect().height > 32)
        .map((el) => el.textContent),
    ),
  );
  expect(tall, "cells wrapped onto a second line").toEqual([]);
  expect(await horizontalOverflow(page)).toEqual([]);
}

/** Only where things sit across: a detail line that changes (the hypervisor picked automatically
 *  until one is chosen) may wrap differently and change a row's height, never a column. */
const across = (l: Awaited<ReturnType<typeof layout>>) =>
  Object.fromEntries(Object.entries(l).map(([k, v]) => [k, { name: [v.name.x, v.name.width], status: [v.status.x, v.status.width] }]));

/** Makes `to` the default with its row's "Use by default", from `from`: the mark moves, nothing
 *  else (on one line per detail, as at 1280px in English, not even down). */
async function switchDefault(page: Page, s: Strings, from: string, to: string, exact: boolean) {
  const before = await layout(page);
  await row(page, to).getByRole("button", { name: s.screen.useDefault }).click();
  await expect(slot(row(page, to), "mark")).toHaveText(s.screen.defaultMark);
  await expect(row(page, from).getByRole("button", { name: s.screen.useDefault })).toBeVisible();
  await animationsFinished(page);
  const after = await layout(page);
  expect(across(after)).toEqual(across(before));
  if (exact) expect(after).toEqual(before);
}

for (const [locale, s, viewport] of [
  ["en", en, { width: 1280, height: 900 }],
  ["de", de, { width: 960, height: 900 }],
  ["fr", fr, { width: 960, height: 900 }],
] as [Locale, Strings, { width: number; height: number }][]) {
  test.describe(`the Machine page's providers (${locale}, ${viewport.width}px)`, () => {
    test.use({ appLocale: locale, viewport });

    test("line up in columns, and the default moves without moving a name or a status", async ({ page }) => {
      await openMachine(page, s);
      await expectAligned(page);
      // Both ready engines and both ready hypervisors say the same word; only the slot differs.
      for (const name of ["Docker Desktop", "OrbStack", "UTM", "Parallels"]) await expect(slot(row(page, name), "status")).toHaveText(s.screen.status.ready);
      await expect(slot(row(page, "Colima"), "status")).toHaveText(s.screen.status.stopped);
      await expect(slot(row(page, "VMware Fusion"), "status")).toHaveText(s.screen.status.needsSetup);
      await expect(providers(page).locator('[data-slot="mark"]')).toHaveCount(2);

      // A hypervisor (the Settings choice) and an engine (Docker pointed at it, the machine re-read).
      await switchDefault(page, s, "UTM", "Parallels", locale === "en");
      await switchDefault(page, s, "Docker Desktop", "OrbStack", locale === "en");
      await expectAligned(page);
    });
  });
}

test.describe("the Machine page's providers, more", () => {
  test("opening a row's test keeps every column where it was", async ({ page }) => {
    await openMachine(page, en);
    const before = await layout(page);
    const xs = await columnXs(page, "actions");
    const test = row(page, "OrbStack").getByRole("button", { name: "Test", exact: true });
    const width = (await test.boundingBox())?.width;
    await test.click();
    const hide = row(page, "OrbStack").getByRole("button", { name: "Hide test", exact: true });
    await expect(hide).toBeVisible();
    // Same width as Test (it kept room for this label), so the action column didn't grow.
    expect((await hide.boundingBox())?.width).toBe(width);
    expect(await columnXs(page, "actions")).toEqual(xs);
    // The rows above the open test stay put; the status column's x for every row too.
    const after = await layout(page);
    for (const name of ["Docker Desktop", "OrbStack"]) expect(after[name]).toEqual(before[name]);
    expect(new Set(Object.values(after).map((r) => r.status.x))).toEqual(new Set([before["UTM"].status.x]));
  });

  test("each status says what it means in a tooltip, on hover and on keyboard focus", async ({ page }) => {
    await openMachine(page, en);
    const status = slot(row(page, "Colima"), "status");
    const tip = row(page, "Colima").getByRole("tooltip").first();
    await expect(tip).toHaveText(en.screen.statusHint.stopped);
    await expect(tip).toHaveCSS("opacity", "0");
    await status.focus();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Tab");
    await expect(status).toBeFocused();
    await expect(tip).toHaveCSS("opacity", "1");
    // Not clipped: the last row's tooltip reaches below the panel, and no box around it cuts it.
    await slot(row(page, "Parallels"), "status").hover();
    const last = row(page, "Parallels").getByRole("tooltip").first();
    await expect(last).toHaveText(en.screen.statusHint.ready.vm);
    await expect(last).toHaveCSS("opacity", "1");
    const clippedBy = await last.evaluate((tip) => {
      const r = tip.getBoundingClientRect();
      const cuts: string[] = [];
      for (let el = tip.parentElement; el && el.tagName !== "MAIN"; el = el.parentElement) {
        const b = el.getBoundingClientRect();
        const clips = getComputedStyle(el).overflow !== "visible";
        if (clips && (r.bottom > b.bottom + 0.5 || r.left < b.left - 0.5 || r.right > b.right + 0.5)) cuts.push(el.className);
      }
      return cuts;
    });
    expect(clippedBy).toEqual([]);
  });
});
