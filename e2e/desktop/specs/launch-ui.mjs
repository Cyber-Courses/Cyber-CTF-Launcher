// Happy path through the REAL import launch (frontend-tracked): start a local lab through the
// `__wdio_launch_local__` hook (src/components/wdio-launch.tsx), reach running on the Overview.
import { existsSync } from "node:fs";
import { finish, goTo, openApp, waitForLab } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, 220000, "lab not running");
  console.log("LAUNCH_UI_OK the real import launch reached running");
} catch (e) {
  console.log("LAUNCH_UI_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
