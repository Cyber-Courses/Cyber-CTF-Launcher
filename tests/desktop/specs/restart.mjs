// Functional GUI e2e: a running lab survives an app restart. Launch with the seed, verify running,
// kill the app, reopen WITHOUT the seed, and assert the lab is rediscovered as running (the
// running-labs scan, not a cache: the in-memory state died with the first process).
import { finish, goTo, LAB_START_MS, localLab, mainText, openApp, results, sleep, stopApp, waitForLab } from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let app1;
let app2;
let b2;
try {
  const first = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } });
  app1 = first.proc;
  await goTo(first.b, "Overview");
  await waitForLab(first.b, LAB_START_MS);
  console.log("phase1: running after seed");
  await finish(first.b, app1);
  app1 = null;
  await sleep(4000);

  ({ proc: app2, b: b2 } = await openApp());
  await goTo(b2, "Overview");
  await waitForLab(b2, 70000);
  r.pass("restart", "lab still running after app restart");
} catch (e) {
  r.fail("restart", e.message);
  if (b2) console.log("main:", JSON.stringify((await mainText(b2)).slice(0, 400)));
} finally {
  await finish(b2, app2);
  stopApp(app1);
}
r.exit();
