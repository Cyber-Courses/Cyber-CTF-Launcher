// Functional: stop a running lab from the Machine > Running now panel; it must stop and clear.
import { finish, goTo, LAB_START_MS, labRunning, localLab, localLabName, mainText, openApp, results, waitForLab } from "../lib/support.mjs";

/** The name the Running now panel gives a local lab (prettyLabId of its `local-<slug>` id). */
const prettyName = (lab) =>
  `local-${localLabName(lab)}`
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, LAB_START_MS, "lab not running");
  await goTo(b, "Machine");
  // The Stop of this lab's own row: other labs may run here too (each row is its name, then Stop).
  const stopBtn = await b.$(`//main//div[div/p[normalize-space()=${JSON.stringify(prettyName(LAB))}]]/button[normalize-space()="Stop"]`);
  await stopBtn.waitForClickable({ timeout: 30000 });
  console.log(`clicking Stop on ${prettyName(LAB)}`);
  await stopBtn.click();
  await goTo(b, "Overview");
  await b.waitUntil(async () => /Nothing running/i.test(await mainText(b)) || !(await labRunning(b)), {
    timeout: 60000,
    interval: 3000,
    timeoutMsg: "lab did not stop",
  });
  r.pass("stop", "the running lab stopped and cleared");
} catch (e) {
  r.fail("stop", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
