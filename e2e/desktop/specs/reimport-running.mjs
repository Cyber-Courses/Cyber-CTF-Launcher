// Error/edge: importing a lab that is already running must be handled gracefully — no crash, no
// second copy, the app stays usable and exactly one copy runs. Launch the local lab, then invoke
// lab_launch_local on the SAME folder again; whatever the command decides (refuse, or restart in
// place), the machine must not end up with two copies and the shell must stay usable.
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { finish, goTo, invokeCmd, labRunning, mainText, openApp, sleep, waitForLab } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

const containers = (id) => execSync(`docker ps --format '{{.Names}}' --filter name=^cyberctf-${id}`, { encoding: "utf8" }).split("\n").filter(Boolean);

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
  console.log(`REIMPORT_RUNNING_OK re-import handled gracefully (refused=${refused}, containers ${before.length}->${after.length}, app usable)`);

  await invokeCmd(b, "lab_stop", { id, runtime: "DOCKER" }, { streaming: true });
} catch (e) {
  console.log("REIMPORT_RUNNING_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
