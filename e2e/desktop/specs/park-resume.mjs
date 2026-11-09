// Normal lifecycle: a running container lab parks and resumes. Launch the local lab, confirm it
// runs, lab_park{pause} and confirm nothing of the lab is running any more (Docker park stops the
// containers, keeping them), lab_resume and confirm it runs again, then stop. Drives the real
// lab_park / lab_resume commands (the lab page's Pause / Resume) against a local lab, which has no
// detail-page buttons of its own.
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { finish, goTo, invokeWhenIdle, mainText, openApp, sleep, waitForLab, waitSettled } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

// The lab has one-shot init/probe containers that exit by design, so count running containers
// rather than expect them all running: park leaves none running (stopped or paused), resume runs
// the service container(s) again.
const states = (id) => execSync(`docker ps -a --format '{{.State}}' --filter name=^cyberctf-${id}`, { encoding: "utf8" }).split("\n").filter(Boolean);
const runningCount = (s) => s.filter((x) => x === "running").length;

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, 300000, "lab never ran");
  const main = await mainText(b);
  const raw = (main.match(/local-[a-z0-9_-]+/) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  const before = states(id);
  if (runningCount(before) === 0) throw new Error(`no running container before park: ${JSON.stringify(before)}`);

  // The launch worker holds the per-lab lock a little past the point the Overview shows "running";
  // wait for it to settle so park isn't refused for an operation already in progress.
  await waitSettled(b, id);
  await invokeWhenIdle(b, "lab_park", { id, runtime: "DOCKER", mode: "pause" });
  await sleep(1500);
  const parked = states(id);
  if (runningCount(parked) !== 0) throw new Error(`lab_park left container(s) running: ${JSON.stringify(parked)}`);
  console.log("parked (nothing running); states:", JSON.stringify(parked));

  await invokeWhenIdle(b, "lab_resume", { id, runtime: "DOCKER" });
  await sleep(1500);
  const resumed = states(id);
  if (runningCount(resumed) === 0) throw new Error(`lab_resume did not bring the lab back: ${JSON.stringify(resumed)}`);
  console.log("PARK_RESUME_OK parked then resumed; states:", JSON.stringify(resumed));

  // Teardown only; the verdict above already stands. run.sh's cleanup_lab tears the lab down
  // regardless, so a stop that races the resume's lock here is not a failure.
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" }).catch((e) => console.log("(cleanup lab_stop skipped:", e.message + ")"));
} catch (e) {
  console.log("PARK_RESUME_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
