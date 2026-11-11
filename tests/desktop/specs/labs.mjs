// The Labs screen, a catalogue lab's page and the labs imported from a folder. The list: the
// catalogue read (signed in or not, browsing needs no account), its search narrowing it and saying
// when nothing matches. A catalogue lab's page: its title, its brief, where it can run. Then the
// real "Import a lab" button, the system's folder picker answered with the lab folder
// (`answerPickers`): the lab starts, the "Imported from a folder" list holds it, its page shows
// what its `.ctf/` folder defines, it runs, and Stop & remove (its confirmation answered) stops it
// and keeps it listed. Starting a catalogue lab is the lab-form-* cases'.
//
//   CYBERCTF_TEST_CATALOGUE_LAB  the catalogue lab whose page to open (default Supplier portal API)
//   CYBERCTF_TEST_LOCAL_LAB      the lab folder to import (none: the import checks skipped)
import { existsSync } from "node:fs";
import { join } from "node:path";
import {
  CATALOGUE_LAB,
  answerPickers,
  dockerUp,
  finish,
  goTo,
  importedLabs,
  localLab,
  localLabName,
  mainText,
  openApp,
  openLabFromList,
  results,
  sleep,
  stopAndRemoveOnPage,
  waitLabPageRunning,
} from "../lib/support.mjs";

const r = results();
const LAB = localLab();

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Labs");
  let listed = false;
  await r.step("list", "the Labs screen reads the catalogue", async () => {
    await b.waitUntil(async () => /in the catalogue|Can.t reach the lab catalogue/.test(await mainText(b)), {
      timeout: 60000,
      timeoutMsg: "the catalogue never answered",
    });
    const text = await mainText(b);
    if (/Can.t reach the lab catalogue/.test(text)) throw new Error("the catalogue can't be reached from this machine");
    listed = true;
    return text.split("\n").find((l) => /in the catalogue/.test(l)) ?? "listed";
  });

  if (listed) {
    const search = await b.$('main input[aria-label="Search labs and skills"]');
    await r.step("search", `the search narrows the list to "${CATALOGUE_LAB}", and says when nothing matches`, async () => {
      await search.setValue(CATALOGUE_LAB);
      await b.waitUntil(async () => (await b.$$("main button[data-row-open]")).length >= 1, { timeout: 15000, timeoutMsg: "no row left for that search" });
      await search.setValue("zz-no-such-lab-zz");
      await b.waitUntil(async () => /No labs match/.test(await mainText(b)), { timeout: 15000, timeoutMsg: "no 'No labs match' for a search nothing matches" });
      await search.setValue("");
    });
    if (await openLabFromList(b, CATALOGUE_LAB)) {
      await r.step("catalogue-page", `${CATALOGUE_LAB}'s page shows its brief and where it runs`, async () => {
        await b.waitUntil(async () => /Brief/.test(await mainText(b)) && /Where it runs|Details/.test(await mainText(b)), {
          timeout: 30000,
          timeoutMsg: "no Brief and Where it runs panels",
        });
        const text = await mainText(b);
        return `${CATALOGUE_LAB}: ${/Sign in to start/.test(text) ? "signed out, Sign in to start offered" : "Start lab offered"}`;
      });
    } else r.skip("catalogue-page", `the catalogue doesn't list "${CATALOGUE_LAB}" (set CYBERCTF_TEST_CATALOGUE_LAB)`);
  } else {
    r.skip("search", "no catalogue to search");
    r.skip("catalogue-page", "no catalogue");
  }

  if (!LAB) {
    for (const id of ["import", "imported-list", "imported-page", "imported-runs", "imported-stop"]) r.skip(id, "no lab folder: set CYBERCTF_TEST_LOCAL_LAB");
  } else if (!dockerUp()) {
    for (const id of ["import", "imported-list", "imported-page", "imported-runs", "imported-stop"])
      r.skip(id, "no Docker engine answers here (an imported lab runs as containers)");
  } else {
    const id = `local-${localLabName(LAB)}`;
    await goTo(b, "Labs");
    await answerPickers(b, LAB);
    const imported = await r.step("import", "Import a lab, the folder picked, starts the lab", async () => {
      const button = await b.$('//main//button[contains(normalize-space(), "Import a lab")]');
      await button.waitForClickable({ timeout: 30000 });
      await button.click();
      await b.waitUntil(async () => /Importing/.test(await mainText(b)) || /Lab imported and starting/.test(await b.$("body").getText()), {
        timeout: 30000,
        timeoutMsg: "the import never started",
      });
    });
    let title = null;
    await r.step("imported-list", "the lab is listed under Imported from a folder", async () => {
      await b.waitUntil(async () => (title = (await importedLabs(b)).find((l) => l.id === id)?.title ?? null) !== null, {
        timeout: 300000,
        interval: 3000,
        timeoutMsg: `no imported lab ${id}`,
      });
      await goTo(b, "Labs");
      await b.waitUntil(async () => /Imported from a folder/.test(await mainText(b)), { timeout: 30000, timeoutMsg: "no Imported from a folder list" });
      return `${title} (${id})`;
    });
    if (imported && title) {
      await r.step("imported-page", "its page opens from the list, with what its .ctf/ folder defines", async () => {
        const open = await b.$(`main button[aria-label=${JSON.stringify(`Open ${title}`)}]`);
        await open.waitForClickable({ timeout: 30000 });
        await open.click();
        await b.waitUntil(
          async () =>
            (
              await b
                .$("main h1")
                .getText()
                .catch(() => "")
            ).includes(title),
          { timeout: 30000, timeoutMsg: "its page never opened" },
        );
        if (existsSync(join(LAB, ".ctf", "objectives.json")) && !/Objectives/.test(await mainText(b)))
          throw new Error("the lab has .ctf/objectives.json but its page shows no Objectives");
      });
      await r.step("imported-runs", "the imported lab runs, as its page says", () => waitLabPageRunning(b, 900000));
      await r.step("imported-stop", "Stop & remove stops it (confirmed in the system dialog) and it stays listed", async () => {
        await stopAndRemoveOnPage(b);
        await sleep(2000);
        if (!(await importedLabs(b)).some((l) => l.id === id)) throw new Error("the lab is no longer installed after its stop");
      });
    } else {
      for (const c of ["imported-page", "imported-runs", "imported-stop"]) r.skip(c, "the import didn't go through");
    }
  }
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
