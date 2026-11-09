// Failure scenario: a bad lab folder (not a lab) must be handled gracefully: the app stays usable
// and nothing ends up running (no crash, no phantom lab).
import { finish, goTo, mainText, openApp, sleep } from "../support.mjs";

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: "/tmp/not-a-lab-folder-xyz" } }));
  await goTo(b, "Overview");
  await sleep(10000); // let the bad launch attempt settle
  const usable = (await b.$("main h1").isExisting()) && (await b.$("aside nav").isExisting());
  const main = await mainText(b);
  const noRunning = /Nothing running/i.test(main) || !/\blab[s]? running on this machine\b/i.test(main);
  if (usable && noRunning) console.log("FAIL_BADLAB_OK a bad lab folder is handled gracefully; nothing runs");
  else {
    console.log("FAIL_BADLAB_FAIL usable=", usable, "noRunning=", noRunning, "main=", JSON.stringify(main.slice(0, 200)));
    rc = 1;
  }
} catch (e) {
  console.log("FAIL_BADLAB_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
