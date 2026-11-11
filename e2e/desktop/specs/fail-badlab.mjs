// Failure scenario: a bad lab folder (not a lab) must be handled gracefully: the app stays usable
// and nothing ends up running (no crash, no phantom lab).
import { finish, goTo, mainText, openApp, results, sleep } from "../support.mjs";

const r = results();
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: "/tmp/not-a-lab-folder-xyz" } }));
  await goTo(b, "Overview");
  await sleep(10000); // let the bad launch attempt settle
  const usable = (await b.$("main h1").isExisting()) && (await b.$("aside nav").isExisting());
  const main = await mainText(b);
  const noRunning = /Nothing running/i.test(main) || !/\blab[s]? running on this machine\b/i.test(main);
  if (usable && noRunning) r.pass("fail-badlab", "a bad lab folder is handled gracefully; nothing runs");
  else {
    r.fail("fail-badlab", `usable: ${usable}, nothing running: ${noRunning}; main: ${main.slice(0, 200)}`);
  }
} catch (e) {
  r.fail("fail-badlab", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
