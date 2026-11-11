// Resilience (tests/resilience.md): the deploy worker killed mid-start (SIGKILL, a crash, the
// Activity Monitor), the app still open. The start says it stopped without a result; the lab
// reads as failed, not running on whatever the start had brought up; the worker's job file (it
// holds the launch token) is gone; a Stop clears what it left, and a start then brings it up.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  appDataDir,
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
if (process.platform === "win32") r.skipAll("kills the worker with SIGKILL: Unix only (Windows: taskkill /F, the rig's own case)");
const id = localLabId(LAB);
const deploys = join(appDataDir(), "deploys");

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await invokeInBackground(b, "start", "lab_launch_local", { path: LAB });
  if (!(await startMade(b, id))) throw new Error(`the start never made a container: ${JSON.stringify(await outcomeOf(b, "start", 1000))}`);
  const pid = Number(
    readFileSync(join(deploys, `${id}.pid`), "utf8")
      .trim()
      .split(/\s+/)[0],
  );
  process.kill(pid, "SIGKILL");
  r.pass("kill-worker/killed", `deploy worker ${pid} killed mid-start`);

  const start = await outcomeOf(b, "start", 120000);
  if (start && start.err && /without a result/i.test(start.err)) r.pass("kill-worker/said", start.err);
  else r.fail("kill-worker/said", `the start ended: ${JSON.stringify(start)}`);

  const status = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  if (status && !status.running) r.pass("kill-worker/not-running", "the lab reads as not running");
  else r.fail("kill-worker/not-running", `status: ${JSON.stringify(status)}`);
  if (!existsSync(join(deploys, `${id}.json`))) r.pass("kill-worker/no-token", "the worker's job file is gone");
  else r.fail("kill-worker/no-token", "the killed worker's job file (its launch token) is still there");

  // As the failure says: a Stop clears what the killed start left (a start is refused while
  // something of the lab is there), then a start again brings it up.
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" }, 600000);
  if (labContainers(id).length === 0) r.pass("kill-worker/cleared", "a Stop clears what the killed start left");
  else r.fail("kill-worker/cleared", `left: ${JSON.stringify(labContainers(id))}`);
  await invokeWhenIdle(b, "lab_launch_local", { path: LAB }, 900000);
  await waitSettled(b, id);
  const again = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  if (again && again.running) r.pass("kill-worker/start-again", "a start again brings the lab up");
  else r.fail("kill-worker/start-again", `after a start again: ${JSON.stringify(again)}`);
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" });
  if (labContainers(id).length === 0) r.pass("kill-worker/stopped", "it stops, nothing left");
  else r.fail("kill-worker/stopped", `left: ${JSON.stringify(labContainers(id))}`);
} catch (e) {
  r.fail("kill-worker", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
