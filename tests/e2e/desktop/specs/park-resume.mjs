// Normal lifecycle: a running container lab pauses and resumes, then shuts down and resumes. Launch
// the local lab, confirm it runs and wait until it is as its launch leaves it (the attack box "Start
// it with each lab" adds up too). lab_park{pause} freezes every container that ran (Docker reads
// them `paused`, none running) and the lab reads paused, still after the app is quit and opened
// again; lab_resume brings back the same containers, running as before, and a shell in the attack
// box answers. lab_park{shutdown} then leaves nothing of the lab running (Docker stops the
// containers, keeping them), lab_resume runs it again, then stop. Drives the real lab_park /
// lab_resume commands (the lab page's Pause, Shut down and Resume) against a local lab, which has
// no detail-page buttons of its own.
import {
  containersChanged,
  docker,
  eventually,
  finish,
  goTo,
  invokeWhenIdle,
  LAB_START_MS,
  labContainers,
  localLab,
  mainText,
  openApp,
  results,
  waitForLab,
  waitLaunchSettled,
} from "../support/app.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

// The lab has one-shot init/probe containers that exit by design, so count running containers
// rather than expect them all running: park leaves none running (paused or stopped), resume runs
// the service container(s) again.
const states = (id) => labContainers(id).map((c) => c.state);
const runningCount = (s) => s.filter((x) => x === "running").length;
/** Why `after` isn't `before` paused (each container that ran now `paused`, the others as they
 *  were, none added or gone); null when it is. */
const notAsPaused = (before, after) =>
  containersChanged(
    before.map((c) => (c.state === "running" ? { ...c, state: "paused" } : c)),
    after,
  );
/** Lab `id`'s containers in `cs`, each as `<service> <state>`. */
const seen = (id, cs) => cs.map((c) => `${c.name.replace(`cyberctf-${id}-`, "")} ${c.state}`).join(", ");

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
  // read once both are done, so a container the box adds is never taken for the pause's doing.
  await waitLaunchSettled(b, id);
  const before = labContainers(id);
  if (runningCount(before.map((c) => c.state)) === 0) throw new Error(`no running container before park: ${JSON.stringify(before)}`);

  // Pause: every container that ran is frozen, none stopped or removed, and the lab reads paused.
  await invokeWhenIdle(b, "lab_park", { id, runtime: "DOCKER", mode: "pause" });
  let paused = labContainers(id);
  await eventually(() => runningCount((paused = labContainers(id)).map((c) => c.state)) === 0, 15000, 1000);
  const frozen = notAsPaused(before, paused);
  const read = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  r.check(
    "pause",
    frozen === null && read.parked === "pause" && !read.running,
    frozen ?? `each container that ran is paused; the lab reads ${read.running ? "running" : `parked (${read.parked})`}: ${seen(id, paused)}`,
  );

  // Quit and open the app again: the lab still reads paused, its containers frozen.
  await finish(b, proc);
  ({ proc, b } = await openApp());
  const again = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  const still = notAsPaused(before, labContainers(id));
  r.check(
    "pause-restart",
    still === null && again.parked === "pause" && !again.running,
    still ?? `after the app opened again the lab reads ${again.running ? "running" : `parked (${again.parked})`}`,
  );

  // Resume: the same containers run again, as they were, and a shell in the attack box answers.
  await invokeWhenIdle(b, "lab_resume", { id, runtime: "DOCKER" });
  let back = labContainers(id);
  await eventually(() => containersChanged(before, (back = labContainers(id))) === null, 15000, 1000);
  const changed = containersChanged(before, back);
  const box = before.find((c) => /-attackbox-/.test(c.name) && c.state === "running");
  const shell = box ? (docker("exec", box.name, "sh", "-c", "echo shell-ok") ?? "").trim() : "no attack box";
  r.check(
    "pause-resume",
    changed === null && (!box || shell === "shell-ok"),
    changed ? `resumed, yet the lab isn't as it was: ${changed}` : `resumed as it was (${seen(id, back)}); the attack box's shell: ${shell}`,
  );

  await invokeWhenIdle(b, "lab_park", { id, runtime: "DOCKER", mode: "shutdown" });
  let parked = states(id);
  await eventually(() => runningCount((parked = states(id))) === 0, 15000, 1000);
  if (runningCount(parked) !== 0) throw new Error(`lab_park{shutdown} left container(s) running: ${JSON.stringify(parked)}`);
  console.log("shut down (nothing running); states:", JSON.stringify(parked));

  await invokeWhenIdle(b, "lab_resume", { id, runtime: "DOCKER" });
  let resumed = states(id);
  await eventually(() => runningCount((resumed = states(id))) > 0, 15000, 1000);
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
