// Resilience (tests/resilience.md): Start pressed twice (a double click, or the catalogue's Start
// and the lab page's at once). Exactly one start runs, the other is refused as already in
// progress, and the lab comes up once.
import { finish, invokeInBackground, invokeWhenIdle, labContainers, localLab, localLabId, openApp, outcomeOf, results, waitSettled } from "../lib/support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
const id = localLabId(LAB);

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await invokeInBackground(b, "first", "lab_launch_local", { path: LAB });
  await invokeInBackground(b, "second", "lab_launch_local", { path: LAB });
  const [first, second] = [await outcomeOf(b, "first", 900000), await outcomeOf(b, "second", 900000)];
  const outcomes = [first, second];
  const refused = outcomes.filter((o) => o && o.err && /already in progress/i.test(o.err));
  const ran = outcomes.filter((o) => o && o.ok);
  if (ran.length === 1 && refused.length === 1) r.pass("double-start/one-runs", `one start ran, the other was refused: ${refused[0].err}`);
  else r.fail("double-start/one-runs", `outcomes: ${JSON.stringify(outcomes)}`);

  await waitSettled(b, id);
  const status = await invokeWhenIdle(b, "lab_status", { id, runtime: "DOCKER" });
  if (status && status.running) r.pass("double-start/running", "the lab is up, once");
  else r.fail("double-start/running", `status: ${JSON.stringify(status)}`);
  // One Compose project, each of its containers once: a second start never made a copy.
  const names = labContainers(id).map((c) => c.name);
  if (new Set(names).size === names.length && names.length > 0) r.pass("double-start/one-copy", `containers: ${names.join(", ")}`);
  else r.fail("double-start/one-copy", `containers: ${JSON.stringify(names)}`);
  await invokeWhenIdle(b, "lab_stop", { id, runtime: "DOCKER" });
  if (labContainers(id).length === 0) r.pass("double-start/stopped", "it stops, nothing left");
  else r.fail("double-start/stopped", `left: ${JSON.stringify(labContainers(id))}`);
} catch (e) {
  r.fail("double-start", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
