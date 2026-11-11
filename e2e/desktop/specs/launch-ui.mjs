// Happy path through the REAL import launch (frontend-tracked): start a local lab through the
// `__wdio_launch_local__` hook (src/components/wdio-launch.tsx), reach running on the Overview.
import { finish, goTo, localLab, openApp, results, waitForLab } from "../support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, 220000, "lab not running");
  r.pass("launch-ui", "the real import launch reached running");
} catch (e) {
  r.fail("launch-ui", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
