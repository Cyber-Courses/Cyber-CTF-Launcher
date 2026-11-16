// Resilience (docs/contributing/testing/resilience.md, A3): the runner service, which runs the
// lab's operations, killed mid-start (SIGKILL, a crash, the Activity Monitor), the app still
// open. The start ends with an error, not as started; the lab reads as not running on whatever
// the start had brought up; a Stop clears what it left, and a start then brings it up (the
// service started again on first use).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  appDataDir,
  finish,
  invokeInBackground,
  invokeWhenIdle,
  LAB_START_MS,
  labContainers,
  localLab,
  localLabId,
  openApp,
  outcomeOf,
  results,
  startMade,
  STOP_MS,
  waitSettled,
} from "../support/app.mjs";

export const needs = ["unix", "running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
if (process.platform === "win32") r.skipAll("kills the runner service with SIGKILL: Unix only (Windows: taskkill /F, the rig's own case)");
const id = localLabId(LAB);
// What the runner service of the app's runner home says of itself while it runs (Isothrum's
// service.json): its pid among the rest.
const serviceFile = join(appDataDir(), "isothrum", "service.json");

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await invokeInBackground(b, "start", "lab_launch_local", { path: LAB });
  if (!(await startMade(b, id))) throw new Error(`the start never made a container: ${JSON.stringify(await outcomeOf(b, "start", 1000))}`);
  const { pid } = JSON.parse(readFileSync(serviceFile, "utf8"));
  process.kill(pid, "SIGKILL");
  r.pass("kill-worker/killed", `runner service ${pid} killed mid-start`);

  const start = await outcomeOf(b, "start", STOP_MS);
  if (start && start.err) r.pass("kill-worker/said", `the start ended with: ${start.err}`);
  else r.fail("kill-worker/said", `the start ended: ${JSON.stringify(start)}`);

  const status = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  if (status && !status.running) r.pass("kill-worker/not-running", "the lab reads as not running");
  else r.fail("kill-worker/not-running", `status: ${JSON.stringify(status)}`);

  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" }, STOP_MS);
  if (labContainers(id).length === 0) r.pass("kill-worker/cleared", "a Stop clears what the killed start left");
  else r.fail("kill-worker/cleared", `left: ${JSON.stringify(labContainers(id))}`);
  await invokeWhenIdle(b, "lab_launch_local", { path: LAB }, LAB_START_MS);
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
