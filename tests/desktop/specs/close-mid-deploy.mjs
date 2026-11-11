// Resilience (tests/resilience.md): the window closed while a lab starts. The start goes on in its
// deploy worker (closing the app never cuts a deploy short); the app reopened finds it, shows the
// lab coming up and then running, and stops it cleanly.
import {
  closeWindow,
  eventually,
  finish,
  goTo,
  invokeInBackground,
  invokeWhenIdle,
  labContainers,
  localLab,
  localLabId,
  openApp,
  outcomeOf,
  results,
  startMade,
  stopApp,
  waitForLab,
} from "../lib/support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
const id = localLabId(LAB);

let p1;
let p2;
let b2;
try {
  const first = await openApp();
  p1 = first.proc;
  await invokeInBackground(first.b, "start", "lab_launch_local", { path: LAB });
  if (!(await startMade(first.b, id))) throw new Error(`the start never made a container: ${JSON.stringify(await outcomeOf(first.b, "start", 1000))}`);
  // The window's own close (its red button, Alt+F4): no in-app operation holds it, so it goes.
  await closeWindow(first.b, "main");
  await first.b.deleteSession().catch(() => {});
  const exited = await eventually(() => p1.exitCode !== null, 30000);
  console.log(exited ? "the app exited with its window" : "the app kept running without its window: stopping it");
  stopApp(p1);
  p1 = null;
  r.pass("close-mid-deploy/closed", "the window closed while the lab started");

  ({ proc: p2, b: b2 } = await openApp());
  await goTo(b2, "Overview");
  await waitForLab(b2, 600000, "the reopened app never showed the lab running");
  r.pass("close-mid-deploy/finished", "the start went on without the window, and the reopened app shows the lab running");
  await invokeWhenIdle(b2, "lab_stop", { id, runtime: "DOCKER" });
  if (labContainers(id).length === 0) r.pass("close-mid-deploy/stopped", "it stops, nothing left");
  else r.fail("close-mid-deploy/stopped", `left: ${JSON.stringify(labContainers(id))}`);
} catch (e) {
  r.fail("close-mid-deploy", e.message);
} finally {
  await finish(b2, p2);
  stopApp(p1);
}
r.exit();
