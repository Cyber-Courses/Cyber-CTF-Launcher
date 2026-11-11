// Settings > Maintenance, the cleanup: what still runs here, the lab downloads (container images
// and VM boxes) removed with Clean up, whose system confirmation (`confirmSystemDialogs`) is
// first answered Cancel (nothing removed), then Remove (the panel says what it freed), and a tool
// Cyber CTF installed removed, when one is named. Run last: the labs download again what they
// need on their next start.
//
//   CYBERCTF_TEST_CLEANUP_TOOL  the name of a tool Settings lists as installed by Cyber CTF to remove
//                              (as the list shows it, e.g. "Terraform"); none: that check skipped,
//                              since removing a tool takes it from the machine
import { closeWindow, confirmSystemDialogs, finish, goTo, openApp, results, sleep, switchToWindow } from "../lib/support.mjs";

const r = results();
const TOOL = process.env.CYBERCTF_TEST_CLEANUP_TOOL || "";

const body = (b) =>
  b
    .$("body")
    .getText()
    .catch(() => "");
/** The names the installed tools list shows (its rows' first line). */
const toolNames = async (b) => Promise.all((await b.$$("//li/div/p[1]")).map((p) => p.getText()));

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Settings");
  await switchToWindow(b, "settings");
  await (await b.$('//nav[@aria-label="Settings sections"]//button[contains(normalize-space(), "Maintenance")]')).click();
  await r.step("section", "Maintenance shows what runs and the lab downloads", async () => {
    await b.waitUntil(async () => /Running now/.test(await body(b)) && /Downloads/.test(await body(b)), {
      timeout: 30000,
      timeoutMsg: "no Running now and Downloads panels",
    });
    await b.waitUntil(async () => /of lab downloads|No lab downloads yet|Couldn.t read what Cyber CTF downloaded/.test(await body(b)), {
      timeout: 120000,
      timeoutMsg: "the downloads were never read",
    });
    const t = await body(b);
    if (/Couldn.t read what Cyber CTF downloaded/.test(t)) throw new Error(t.match(/Couldn.t read what Cyber CTF downloaded[^\n]*/)[0]);
    return t.match(/[^\n]*of lab downloads|No lab downloads yet/)?.[0] ?? "read";
  });

  const clean = await b.$('//button[contains(normalize-space(), "Clean up")]');
  if (await clean.isExisting()) {
    const before = (await body(b)).match(/[^\n]*of lab downloads/)?.[0] ?? "";
    await confirmSystemDialogs(b, "cancel");
    await clean.click();
    await sleep(3000);
    r.check("downloads-cancel", !/Freed|Removing/.test(await body(b)), `Clean up answered Cancel removes nothing (${before})`);
    await confirmSystemDialogs(b);
    await clean.click();
    await r.step("downloads", "Clean up, confirmed, removes the lab downloads and says what it freed", async () => {
      await b.waitUntil(async () => /Freed [^\n]+|Nothing could be removed/.test(await body(b)), {
        timeout: 600000,
        interval: 2000,
        timeoutMsg: "Clean up never said what it freed",
      });
      const t = await body(b);
      if (/Couldn.t remove the downloads/.test(t)) throw new Error("Couldn't remove the downloads");
      return `${before} -> ${t.match(/Freed [^\n]+|Nothing could be removed[^\n]*/)[0]}`;
    });
  } else {
    r.skip("downloads-cancel", "no lab downloads here to clean up");
    r.skip("downloads", "no lab downloads here to clean up");
  }

  const listed = await b.$('//*[normalize-space()="Tools Cyber CTF installed"]').isExisting();
  if (!listed) r.skip("tool-remove", "no tool installed by Cyber CTF here");
  else if (!TOOL) r.skip("tool-remove", `name one to remove with CYBERCTF_TEST_CLEANUP_TOOL (listed: ${(await toolNames(b)).join(", ")})`);
  else {
    await r.step("tool-remove", `${TOOL} removed from Settings (confirmed)`, async () => {
      const row = await b.$(`//li[.//p[normalize-space()=${JSON.stringify(TOOL)}]]`);
      if (!(await row.isExisting())) throw new Error(`${TOOL} isn't listed: ${(await toolNames(b)).join(", ")}`);
      await confirmSystemDialogs(b);
      await (await row.$('.//button[contains(normalize-space(), "Remove")]')).click();
      await b.waitUntil(async () => !(await b.$(`//li[.//p[normalize-space()=${JSON.stringify(TOOL)}]]`).isExisting()), {
        timeout: 900000,
        interval: 3000,
        timeoutMsg: `${TOOL} is still listed`,
      });
    });
  }
  await closeWindow(b, "settings");
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("window text:", JSON.stringify((await body(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
