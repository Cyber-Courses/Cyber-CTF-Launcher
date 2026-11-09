// Functional GUI e2e: a running lab survives an app restart. Launch with the seed, verify running,
// kill the app, reopen WITHOUT the seed, and assert the lab is rediscovered as running (the
// running-labs scan, not a cache: the in-memory state died with the first process).
import { existsSync } from "node:fs";
import { finish, goTo, mainText, openApp, sleep, stopApp, waitForLab } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

let rc = 0;
let app1;
let app2;
let b2;
try {
  const first = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } });
  app1 = first.proc;
  await goTo(first.b, "Overview");
  await waitForLab(first.b, 220000);
  console.log("phase1: running after seed");
  await finish(first.b, app1);
  app1 = null;
  await sleep(4000);

  ({ proc: app2, b: b2 } = await openApp());
  await goTo(b2, "Overview");
  await waitForLab(b2, 70000);
  console.log("RESTART_PERSIST_OK lab still running after app restart");
} catch (e) {
  console.log("RESTART_PERSIST_FAIL", e.message);
  if (b2) console.log("main:", JSON.stringify((await mainText(b2)).slice(0, 400)));
  rc = 1;
} finally {
  await finish(b2, app2);
  stopApp(app1);
}
process.exit(rc);
