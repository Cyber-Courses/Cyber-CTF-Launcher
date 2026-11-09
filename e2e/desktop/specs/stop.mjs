// Functional: stop a running lab from the Machine > Running now panel; it must stop and clear.
import { existsSync } from "node:fs";
import { finish, goTo, labRunning, localLabName, mainText, openApp, waitForLab } from "../support.mjs";

/** The name the Running now panel gives a local lab (prettyLabId of its `local-<slug>` id). */
const prettyName = (lab) =>
  `local-${localLabName(lab)}`
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, 220000, "lab not running");
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
  console.log("STOP_OK the running lab stopped and cleared");
} catch (e) {
  console.log("STOP_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
