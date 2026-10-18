import type { Page } from "@playwright/test";
import { test, expect, LOCALES, type Locale, openApp, navButton, pageTitle, rawKeys } from "./fixtures";
import enShell from "../src/messages/en/shell.json";
import frShell from "../src/messages/fr/shell.json";
import esShell from "../src/messages/es/shell.json";
import deShell from "../src/messages/de/shell.json";
import ptBRShell from "../src/messages/pt-BR/shell.json";
import jaShell from "../src/messages/ja/shell.json";
import enSettings from "../src/messages/en/settings.json";
import frSettings from "../src/messages/fr/settings.json";
import esSettings from "../src/messages/es/settings.json";
import deSettings from "../src/messages/de/settings.json";
import ptBRSettings from "../src/messages/pt-BR/settings.json";
import jaSettings from "../src/messages/ja/settings.json";
import enMachine from "../src/messages/en/machine.json";
import frMachine from "../src/messages/fr/machine.json";
import esMachine from "../src/messages/es/machine.json";
import deMachine from "../src/messages/de/machine.json";
import ptBRMachine from "../src/messages/pt-BR/machine.json";
import jaMachine from "../src/messages/ja/machine.json";
import enOnboarding from "../src/messages/en/onboarding.json";
import frOnboarding from "../src/messages/fr/onboarding.json";
import esOnboarding from "../src/messages/es/onboarding.json";
import deOnboarding from "../src/messages/de/onboarding.json";
import ptBROnboarding from "../src/messages/pt-BR/onboarding.json";
import jaOnboarding from "../src/messages/ja/onboarding.json";

// Every page, in every language: no raw message key ("machine.body.pkgmgr.linux") and no
// placeholder left unfilled ("{count}", "<em>") on screen, and no missing-translation warning
// (the fixture fails the test on one).

const SHELL = { en: enShell, fr: frShell, es: esShell, de: deShell, "pt-BR": ptBRShell, ja: jaShell };
const SETTINGS = { en: enSettings, fr: frSettings, es: esSettings, de: deSettings, "pt-BR": ptBRSettings, ja: jaSettings };
const MACHINE = { en: enMachine, fr: frMachine, es: esMachine, de: deMachine, "pt-BR": ptBRMachine, ja: jaMachine };
const ONBOARDING = { en: enOnboarding, fr: frOnboarding, es: esOnboarding, de: deOnboarding, "pt-BR": ptBROnboarding, ja: jaOnboarding };

/** Web addresses the pages show on purpose, which look like a key. */
const ADDRESSES = new Set(["proxmox.com", "brew.sh", "cyberauth.co"]);

/** Text on screen that a translation left unfilled: a `{name}` or ICU placeholder, or a tag. */
function placeholders(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const re = /\{[a-zA-Z_]+(,[^}]*)?\}|<\/?[a-z]+>/;
    const found = new Set<string>();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const text = n.textContent?.trim() ?? "";
      const el = n.parentElement;
      if (text && re.test(text) && el && !el.closest(".xterm, code, pre") && el.checkVisibility()) found.add(text.slice(0, 120));
    }
    return [...found];
  });
}

async function expectTranslated(page: Page, where: string) {
  const keys = (await rawKeys(page)).filter((k) => !ADDRESSES.has(k));
  expect(keys, `raw keys on ${where}`).toEqual([]);
  expect(await placeholders(page), `placeholders on ${where}`).toEqual([]);
}

for (const locale of LOCALES) {
  test.describe(`every page in ${locale}`, () => {
    test.use({ appLocale: locale });
    const shell = SHELL[locale as Locale];
    const settings = SETTINGS[locale as Locale];
    const machine = MACHINE[locale as Locale];
    const onboarding = ONBOARDING[locale as Locale];

    test("the main window's screens and lab pages", async ({ page }) => {
      await openApp(page);
      await expect(page.locator("html")).toHaveAttribute("lang", locale);
      await expectTranslated(page, "Overview");
      for (const tab of [shell.tabs.labs, shell.tabs.machine, shell.tabs.server, shell.tabs.cloud]) {
        await navButton(page, tab).click();
        await expect(pageTitle(page)).toHaveText(tab);
        // The screens fill in from the mock's answers: let them settle.
        await expect(page.locator("main [aria-busy=true]")).toHaveCount(0);
        await expectTranslated(page, tab);
      }

      // Machine, Setup view.
      await navButton(page, shell.tabs.machine).click();
      await page.getByRole("radiogroup", { name: machine.screen.viewLabel }).getByRole("radio", { name: machine.screen.viewSetup }).click();
      await expect(page.locator("main")).toContainText(machine.checklist.title);
      await expectTranslated(page, "Machine setup");

      // A running container lab, a deploying VM lab, a cloud lab, a stopped one.
      await navButton(page, shell.tabs.labs).click();
      for (const lab of ["Invoice portal API", "GOAD Light", "EC2 SSRF", "Exposed dashboard"]) {
        await page.locator("main").getByRole("button", { name: lab }).first().click();
        await expect(pageTitle(page)).toHaveText(lab);
        await expect(page.locator("main aside")).toBeVisible();
        await expectTranslated(page, lab);
        await page.keyboard.press("Escape");
        await expect(pageTitle(page)).toHaveText(shell.tabs.labs);
      }
    });

    test("Settings, the setup windows and the onboarding", async ({ page }) => {
      await page.goto("/?mock&window=settings");
      const sections = page.getByRole("navigation", { name: settings.tabs.label });
      for (const section of ["account", "appearance", "labs", "maintenance", "about"] as const) {
        await sections.getByRole("button", { name: settings.tabs[section] }).click();
        await expect(page.getByRole("heading", { level: 2 }).first()).toBeVisible();
        await expectTranslated(page, `Settings ${section}`);
      }

      for (const url of ["/server-setup?mock", "/server-setup?mock&kind=cloud", "/server-setup?mock&id=pve", "/server-setup?mock&id=aws1"]) {
        await page.goto(url);
        await expect(page.locator("main h1")).toBeVisible();
        await expect(page.getByText(/\d+.*\d+/).first()).toBeVisible();
        await expectTranslated(page, url);
      }

      // A ready Windows machine, every step; then a fresh one's package manager step.
      await page.goto("/?mock&onboarding&os=windows&arch=x86_64");
      await expectTranslated(page, "onboarding welcome");
      await page.getByRole("button", { name: onboarding.welcome.start }).click();
      const forward = page.getByRole("button", { name: onboarding.back, exact: true }).locator("xpath=following::button[1]");
      for (let n = 0; n < 9; n++) {
        await expect(forward).toBeEnabled();
        await expectTranslated(page, `onboarding step ${n + 2}`);
        await forward.click();
      }
      await expect(page.getByRole("button", { name: onboarding.done.browse })).toBeVisible();
      await expectTranslated(page, "onboarding done");

      for (const os of ["windows", "linux", "macos"]) {
        await page.goto(`/?mock&onboarding&machine=fresh&arch=x86_64&os=${os}`);
        await page.getByRole("button", { name: onboarding.welcome.start }).click();
        await forward.click();
        await expect(forward).toBeDisabled();
        await expectTranslated(page, `fresh ${os} package manager step`);
      }
    });
  });
}
