// The attack box from an imported lab's own page: added from its side rail (or already up, when
// Settings starts it with the lab), running with an address on the lab network, its shell opened
// in its own window (`shell-<lab id>`, xterm.js over a pseudo-terminal), where `whoami` answers
// `player`, and, when a shared folder is set, a file put there read at its mount point from inside
// the box. Then the box stopped from the page (its container gone) and the lab stopped. The box on
// the other forms (a VM lab's Kali, the box in the lab's VM, on AWS) is the integration tests'
// `attack-box-*`; this is the window a player types in.
//
//   CYBERCTF_TEST_LOCAL_LAB  the lab folder to import and start (none: skipped)
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  closeWindow,
  dockerUp,
  finish,
  forceVisible,
  importedLabs,
  invokeCmd,
  LAB_START_MS,
  labContainers,
  localLab,
  localLabName,
  mainText,
  openApp,
  openImportedLab,
  results,
  sleep,
  STOP_MS,
  stopAndRemoveOnPage,
  switchToWindow,
  waitAwake,
  waitLabPageRunning,
} from "../lib/support.mjs";
import { terminalText } from "../lib/page.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB");
if (!dockerUp()) r.skipAll("no Docker engine answers here (an imported lab runs as containers)");
const ID = `local-${localLabName(LAB)}`;

/** The attack box panel's text (the side rail's box section). */
const panel = (b) =>
  b
    .$('(//main//aside//h3[normalize-space()="Attack box" or normalize-space()="Attack VM"])[1]/ancestor::*[.//button][1]')
    .getText()
    .catch(() => "");
const sideButton = (b, label) => b.$(`//main//aside//button[contains(normalize-space(), ${JSON.stringify(label)})]`);
/** The shell window's terminal, as text, one row a line (read in the page: the embedded server's
 *  getText runs the rows together, the answer glued to its command). */
const screen = (b) => b.execute(terminalText).catch(() => "");

/** Types `line` and Enter into the terminal as the keyboard does (its input element). */
async function typeLine(b, line) {
  const input = await b.$(".xterm-helper-textarea");
  await input.waitForExist({ timeout: 20000 });
  await input.addValue(`${line}`);
}

/** Writes `line` to this window's shell session through the command the terminal writes keys
 *  with (`terminal_write`), for a webview whose synthetic keys the terminal doesn't take. A window
 *  reaches only its own session, so the first id that takes it is this window's. */
async function writeLine(b, line) {
  for (let session = 0; session < 64; session++) {
    const ok = await invokeCmd(b, "terminal_write", { session, data: `${line}\r` }, { timeoutMs: 5000 })
      .then(() => true)
      .catch(() => false);
    if (ok) return session;
  }
  throw new Error("no shell session in this window took the line");
}

/** Sends `line` to the shell and waits until `seen(screen)`; how it got there. */
async function run(b, line, seen) {
  const shown = () =>
    b
      .waitUntil(async () => seen(await screen(b)), { timeout: 20000, interval: 1000 })
      .then(() => true)
      .catch(() => false);
  await typeLine(b, line).catch(() => {});
  if (await shown()) return "typed";
  await writeLine(b, line);
  if (await shown()) return "written to its session (the typed keys didn't reach the terminal)";
  throw new Error(`no answer to \`${line}\`; the terminal shows:\n${(await screen(b)).slice(-600)}`);
}

let proc;
let b;
let marker = null;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  let title = null;
  await b.waitUntil(async () => (title = (await importedLabs(b)).find((l) => l.id === ID)?.title ?? null) !== null, {
    timeout: LAB_START_MS,
    interval: 3000,
    timeoutMsg: `the lab was never imported as ${ID}`,
  });
  await openImportedLab(b, title);
  await waitLabPageRunning(b, LAB_START_MS);

  await r.step("box-up", "the attack box runs on the lab network, with an address", async () => {
    const start = await sideButton(b, "Start attack box");
    const clicked = await start.isExisting();
    if (clicked) await start.click();
    await waitAwake(b, async () => /Stop attack box/.test(await panel(b)) && (await (await sideButton(b, "Open attacker shell")).isEnabled()), {
      timeout: LAB_START_MS,
      interval: 3000,
      timeoutMsg: "the box never showed running",
    });
    const address = (await panel(b)).match(/\d{1,3}(?:\.\d{1,3}){3}/)?.[0];
    if (!address) throw new Error(`no address in the panel: ${(await panel(b)).replace(/\s+/g, " ")}`);
    if (!labContainers(ID).some((c) => /attackbox/.test(c.name) && c.state === "running")) throw new Error("no running attackbox container of the lab");
    return `the box runs at ${address} (${clicked ? "started from the page" : "started with the lab"})`;
  });

  const shared = await invokeCmd(b, "shared_folder_get").catch(() => null);
  if (shared?.enabled && shared.path) {
    mkdirSync(shared.path, { recursive: true });
    marker = join(shared.path, `cyberctf-e2e-${process.pid}.txt`);
    writeFileSync(marker, "seen-from-the-box\n");
  }

  await (await sideButton(b, "Open attacker shell")).click();
  const opened = await r.step("shell-window", "Open attacker shell opens the shell in its own window", async () => {
    await switchToWindow(b, `shell-${ID}`);
    await b.$(".xterm-rows").waitForExist({ timeout: 30000 });
    await b.waitUntil(async () => /attack box/.test(await b.$("body").getText()) && (await screen(b)).trim().length > 0, {
      timeout: 120000,
      timeoutMsg: `the shell never opened: ${(await b.$("body").getText()).slice(0, 300)}`,
    });
  });
  if (opened) {
    await r.step("whoami", "whoami in the shell answers player", async () => {
      const how = await run(b, "whoami", (s) => s.split("\n").some((l) => l.trim() === "player"));
      return `whoami: player (${how})`;
    });
    if (marker) {
      const file = `${shared.mountPoint.replace(/\/+$/, "")}/${marker.split(/[\\/]/).pop()}`;
      await r.step("shared-folder", "a file put in the shared folder is read inside the box", async () => {
        const how = await run(b, `cat ${file}`, (s) => s.includes("seen-from-the-box"));
        return `cat ${file}: seen-from-the-box (${how})`;
      });
    } else r.skip("shared-folder", "no shared folder set (Settings > Labs)");
  } else {
    r.skip("whoami", "the shell window didn't open");
    r.skip("shared-folder", "the shell window didn't open");
  }
  await closeWindow(b, `shell-${ID}`);
  await forceVisible(b);

  await r.step("box-stop", "Stop attack box takes the box away; the lab keeps running", async () => {
    const stop = await sideButton(b, "Stop attack box");
    await stop.waitForClickable({ timeout: 30000 });
    await stop.click();
    await waitAwake(b, async () => /Start attack box|not started/.test(await panel(b)), {
      timeout: STOP_MS,
      interval: 3000,
      timeoutMsg: "the box never stopped",
    });
    await sleep(2000);
    const left = labContainers(ID).filter((c) => /attackbox/.test(c.name));
    if (left.length) throw new Error(`the box's container is left: ${left.map((c) => `${c.name} ${c.state}`).join(", ")}`);
    if (!(await mainText(b)).match(/\bRunning\b/)) throw new Error("the lab no longer shows running");
  });
  await stopAndRemoveOnPage(b).catch((e) => console.log("(cleanup: Stop & remove:", e.message + ")"));
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  if (marker) rmSync(marker, { force: true });
  await finish(b, proc);
}
r.exit();
