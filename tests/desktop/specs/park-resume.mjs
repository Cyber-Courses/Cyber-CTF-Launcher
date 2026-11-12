// Normal lifecycle: a running container lab shuts down and resumes. Launch the local lab, confirm it
// runs and wait until it is as its launch leaves it (the attack box "Start it with each lab" adds
// up too); a Pause is refused, as the lab page offers none for containers on Docker (they have no
// state to save: lifecycle's park_refusal), and changes nothing of the lab, each container as it
// was; lab_park{shutdown} then leaves nothing of the lab running (Docker stops the containers,
// keeping them), lab_resume runs it again, then stop. Drives the real lab_park / lab_resume
// commands (the lab page's Shut down / Resume) against a local lab, which has no detail-page
// buttons of its own.
import {
  containersChanged,
  finish,
  goTo,
  invokeWhenIdle,
  LAB_START_MS,
  labContainers,
  localLab,
  mainText,
  openApp,
  results,
  sleep,
  waitForLab,
  waitLaunchSettled,
} from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

// The lab has one-shot init/probe containers that exit by design, so count running containers
// rather than expect them all running: park leaves none running (stopped or paused), resume runs
// the service container(s) again.
const states = (id) => labContainers(id).map((c) => c.state);
const runningCount = (s) => s.filter((x) => x === "running").length;

let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, LAB_START_MS, "lab never ran");
  const main = await mainText(b);
  const raw = (main.match(/local-[a-z0-9_-]+/) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  // The launch worker holds the per-lab lock a little past the point the Overview shows "running",
  // and the attack box "Start it with each lab" adds once it ends joins the lab after that: the lab
  // read once both are done, so a container the box adds is never taken for the refusal's doing.
  await waitLaunchSettled(b, id);
  const before = labContainers(id);
  if (runningCount(before.map((c) => c.state)) === 0) throw new Error(`no running container before park: ${JSON.stringify(before)}`);

  const refusal = await invokeWhenIdle(b, "lab_park", { id, runtime: "DOCKER", mode: "pause" }).then(
    () => null,
    (e) => e.message,
  );
  const afterPause = labContainers(id);
  const changed = containersChanged(before, afterPause);
  const seen = afterPause.map((c) => `${c.name.replace(`cyberctf-${id}-`, "")} ${c.state}`).join(", ");
  const refusedAsPause = refusal !== null && /shut it down instead/i.test(refusal);
  r.check(
    "pause-refused",
    refusedAsPause && changed === null,
    refusal === null
      ? `a Pause of containers on Docker was carried out: ${seen}`
      : !refusedAsPause
        ? `refused, but not as a Pause of containers on Docker is: ${refusal}`
        : changed
          ? `refused (${refusal}), yet the lab changed: ${changed}`
          : `refused (${refusal}); nothing of the lab changed: ${seen}`,
  );

  await invokeWhenIdle(b, "lab_park", { id, runtime: "DOCKER", mode: "shutdown" });
  await sleep(1500);
  const parked = states(id);
  if (runningCount(parked) !== 0) throw new Error(`lab_park{shutdown} left container(s) running: ${JSON.stringify(parked)}`);
  console.log("shut down (nothing running); states:", JSON.stringify(parked));

  await invokeWhenIdle(b, "lab_resume", { id, runtime: "DOCKER" });
  await sleep(1500);
  const resumed = states(id);
  if (runningCount(resumed) === 0) throw new Error(`lab_resume did not bring the lab back: ${JSON.stringify(resumed)}`);
  r.pass("park-resume", `shut down (nothing running), then resumed; states: ${resumed.join(", ")}`);

  // Teardown only; the verdict above already stands. run.sh's cleanup_lab tears the lab down
  // regardless, so a stop that races the resume's lock here is not a failure.
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" }).catch((e) => console.log("(cleanup lab_stop skipped:", e.message + ")"));
} catch (e) {
  r.fail("park-resume", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
