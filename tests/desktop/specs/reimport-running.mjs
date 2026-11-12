// Error/edge: importing a lab that is already running must be handled gracefully — no crash, no
// second copy, the app stays usable and exactly one copy runs. Launch the local lab, then invoke
// lab_launch_local on the SAME folder again; whatever the command decides (refuse, or restart in
// place), the machine must not end up with two copies and the shell must stay usable.
import {
  finish,
  goTo,
  invokeCmd,
  invokeWhenIdle,
  LAB_START_MS,
  labRunning,
  labServices,
  localLab,
  mainText,
  openApp,
  results,
  secondCopy,
  STOP_MS,
  waitForLab,
  waitSettled,
} from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

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
  // The launch's worker done. The attack box "Start it with each lab" attaches next holds the lab
  // a while: the import below waits for the lab to be free (invokeWhenIdle), as the UI's buttons
  // do, so it is refused (or not) for the lab running, never for being busy.
  await waitSettled(b, id, LAB_START_MS);
  const before = labServices(id);

  // Import the same folder again while it runs, once nothing else holds the lab.
  let refused = false;
  try {
    await invokeWhenIdle(b, "lab_launch_local", { path: LAB }, LAB_START_MS);
  } catch (e) {
    refused = true;
    console.log("second import refused (acceptable):", e.message.slice(0, 160));
  }
  await waitSettled(b, id, STOP_MS);

  // The app is still usable and exactly one copy runs.
  await goTo(b, "Machine");
  await goTo(b, "Overview");
  if (!(await b.$("aside nav").isExisting())) throw new Error("shell unusable after the re-import");
  if (!(await labRunning(b))) throw new Error("the lab no longer shows running after the re-import");
  // One copy: one Compose project, each service once (a copy may gain the attack box meanwhile).
  const after = labServices(id);
  const two = secondCopy(after);
  const names = (cs) => JSON.stringify(cs.map((c) => c.name));
  if (two) throw new Error(`a second copy started: ${two} (before ${names(before)}, after ${names(after)})`);
  r.pass("reimport-running", `re-import handled gracefully (refused=${refused}, containers ${before.length}->${after.length}, one copy, app usable)`);

  await invokeCmd(b, "lab_stop", { id, runtime: "DOCKER" }, { streaming: true });
} catch (e) {
  r.fail("reimport-running", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
