// Resilience: killing the app mid-deploy must not corrupt it; reopening renders a coherent state
// (the deploy worker's pidfile and status are swept, the shell comes up, no error screen).
import { appEnds, eventually, finish, goTo, holdsFor, invokeCmd, localLab, localLabId, mainText, openApp, results, stopApp } from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let p1;
let p2;
let b2;
try {
  const first = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } });
  p1 = first.proc;
  // The seed's deploy is under way once the app holds an operation on the lab (busy_labs).
  const id = localLabId(LAB);
  const busy = async () => ((await invokeCmd(first.b, "busy_labs").catch(() => [])) ?? []).includes(id);
  if (!(await eventually(busy, 60000, 1000))) throw new Error(`the seed's deploy of ${id} never showed as under way (busy_labs)`);
  console.log("killing app mid-deploy");
  await finish(first.b, p1);
  if (!(await appEnds(p1, 20000))) throw new Error("the app killed mid-deploy never ended");
  p1 = null;

  ({ proc: p2, b: b2 } = await openApp());
  await goTo(b2, "Overview");
  // Rendered, not crashed: a title and the sidebar, and no error screen, on every look while the
  // reopened app sweeps the killed deploy's pidfile and status.
  let coherent = false;
  let noError = false;
  const sound = async () => {
    const h1 = await b2
      .$("main h1")
      .getText()
      .catch(() => "");
    coherent = h1.length > 0 && (await b2.$("aside nav").isExisting());
    noError = !/something went wrong|uncaught|crash/i.test(await mainText(b2));
    return coherent && noError;
  };
  const rendered = await eventually(sound, 30000);
  if (rendered && (await holdsFor(sound, 6000))) r.pass("kill-mid-deploy", "reopened coherently after a mid-deploy kill");
  else {
    r.fail("kill-mid-deploy", `rendered coherently: ${coherent}, no error screen: ${noError}`);
  }
} catch (e) {
  r.fail("kill-mid-deploy", e.message);
} finally {
  await finish(b2, p2);
  stopApp(p1);
}
r.exit();
