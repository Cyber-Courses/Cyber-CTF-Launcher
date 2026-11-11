// Error/edge: importing a lab that is already running must be handled gracefully — no crash, no
// second copy, the app stays usable and exactly one copy runs. Launch the local lab, then invoke
// lab_launch_local on the SAME folder again; whatever the command decides (refuse, or restart in
// place), the machine must not end up with two copies and the shell must stay usable.
import { finish, goTo, invokeCmd, labContainers, labRunning, localLab, mainText, openApp, results, sleep, waitForLab } from "../lib/support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

const containers = (id) =>
  labContainers(id)
    .filter((c) => c.state === "running")
    .map((c) => c.name);

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
  const before = containers(id);

  // Import the same folder again while it runs.
  let refused = false;
  try {
    await invokeCmd(b, "lab_launch_local", { path: LAB }, { streaming: true, timeoutMs: 180000 });
  } catch (e) {
    refused = true;
    console.log("second import refused (acceptable):", e.message.slice(0, 160));
  }
  await sleep(2500);

  // The app is still usable and exactly one copy runs.
  await goTo(b, "Machine");
  await goTo(b, "Overview");
  if (!(await b.$("aside nav").isExisting())) throw new Error("shell unusable after the re-import");
  if (!(await labRunning(b))) throw new Error("the lab no longer shows running after the re-import");
  const after = containers(id);
  if (after.length > before.length) throw new Error(`a second copy started: ${before.length} -> ${after.length} containers (${JSON.stringify(after)})`);
  r.pass("reimport-running", `re-import handled gracefully (refused=${refused}, containers ${before.length}->${after.length}, app usable)`);

  await invokeCmd(b, "lab_stop", { id, runtime: "DOCKER" }, { streaming: true });
} catch (e) {
  r.fail("reimport-running", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
