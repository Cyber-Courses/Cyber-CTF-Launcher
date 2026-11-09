// Resilience: killing the app mid-deploy must not corrupt it; reopening renders a coherent state
// (the deploy worker's pidfile and status are swept, the shell comes up, no error screen).
import { existsSync } from "node:fs";
import { finish, goTo, mainText, openApp, sleep, stopApp } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

let rc = 0;
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
  if (coherent && noError) console.log("KILL_MIDDEPLOY_OK reopened coherently after a mid-deploy kill");
  else {
    console.log("KILL_MIDDEPLOY_FAIL coherent=", coherent, "noError=", noError);
    rc = 1;
  }
} catch (e) {
  console.log("KILL_MIDDEPLOY_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b2, p2);
  stopApp(p1);
}
process.exit(rc);
