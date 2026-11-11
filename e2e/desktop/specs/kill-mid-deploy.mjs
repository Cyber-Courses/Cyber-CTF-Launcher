// Resilience: killing the app mid-deploy must not corrupt it; reopening renders a coherent state
// (the deploy worker's pidfile and status are swept, the shell comes up, no error screen).
import { finish, goTo, localLab, mainText, openApp, results, sleep, stopApp } from "../support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let p1;
let p2;
let b2;
try {
  const first = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } });
  p1 = first.proc;
  await sleep(10000); // the seed's deploy is under way
  console.log("killing app mid-deploy");
  await finish(first.b, p1);
  p1 = null;
  await sleep(4000);

  ({ proc: p2, b: b2 } = await openApp());
  await goTo(b2, "Overview");
  await sleep(6000);
  const h1 = await b2
    .$("main h1")
    .getText()
    .catch(() => "");
  const coherent = h1.length > 0 && (await b2.$("aside nav").isExisting()); // rendered, not crashed
  const main = await mainText(b2);
  const noError = !/something went wrong|uncaught|crash/i.test(main);
  if (coherent && noError) r.pass("kill-mid-deploy", "reopened coherently after a mid-deploy kill");
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
