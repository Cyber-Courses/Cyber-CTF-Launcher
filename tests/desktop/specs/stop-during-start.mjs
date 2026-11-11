// Resilience (tests/resilience.md): Stop pressed while the lab starts. The stop ends the start's
// worker (and what it was running), takes the lab down, and says it stopped; the start ends
// without calling the lab running; nothing of the lab is left on Docker; and a start again then
// brings it up, as the player would try next.
import {
  finish,
  invokeInBackground,
  invokeWhenIdle,
  labContainers,
  localLab,
  localLabId,
  openApp,
  outcomeOf,
  results,
  startMade,
  waitSettled,
} from "../lib/support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
const id = localLabId(LAB);

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await invokeInBackground(b, "start", "lab_launch_local", { path: LAB });
  // The start is under way once Docker holds a container of the lab.
  if (!(await startMade(b, id))) throw new Error(`the start never made a container: ${JSON.stringify(await outcomeOf(b, "start", 1000))}`);
  r.pass("stop-during-start/started", `the start made ${labContainers(id).length} container(s)`);

  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" });
  r.pass("stop-during-start/stopped", "Stop went through while the lab started");

  const start = await outcomeOf(b, "start", 120000);
  if (!start) r.fail("stop-during-start/start-ended", "the start was still running after its lab was stopped");
  else r.pass("stop-during-start/start-ended", start.err ? `the start ended, cut short: ${start.err}` : "the start ended before the stop took it down");

  const status = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" }).catch(() => null);
  if (status && status.running) r.fail("stop-during-start/not-running", "the lab still reads as running");
  else r.pass("stop-during-start/not-running", "the lab reads as not running");
  const left = labContainers(id);
  if (left.length) r.fail("stop-during-start/nothing-left", `containers left: ${JSON.stringify(left)}`);
  else r.pass("stop-during-start/nothing-left", "nothing of the lab is left on Docker");

  await invokeWhenIdle(b, "lab_launch_local", { path: LAB }, 900000);
  await waitSettled(b, id);
  const again = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  if (again && again.running) r.pass("stop-during-start/start-again", "a start again brings the lab up");
  else r.fail("stop-during-start/start-again", `after a start again: ${JSON.stringify(again)}`);
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" }).catch((e) => console.log("(cleanup lab_stop:", e.message + ")"));
} catch (e) {
  r.fail("stop-during-start", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
