// Failure scenario: a bad lab folder (not a lab) must be handled gracefully: the app stays usable
// and nothing ends up running (no crash, no phantom lab).
import { eventually, finish, goTo, holdsFor, LAB_RUNNING, mainText, openApp, results } from "../lib/support.mjs";

export const needs = [];
const r = results();
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: "/tmp/not-a-lab-folder-xyz" } }));
  await goTo(b, "Overview");
  // The launch is the Import button's (WdioLaunch): its refusal is the "Couldn't import" toast.
  const refused = await eventually(
    async () =>
      /Couldn.t import/i.test(
        await b
          .$("body")
          .getText()
          .catch(() => ""),
      ),
    30000,
  );
  // Then nothing may run, on any look while the Overview's status polls catch up.
  let main = "";
  const nothingRuns = async () => {
    main = await mainText(b);
    return /Nothing running/i.test(main) || !LAB_RUNNING.test(main);
  };
  const noRunning = await holdsFor(nothingRuns, 10000);
  const usable = (await b.$("main h1").isExisting()) && (await b.$("aside nav").isExisting());
  if (usable && noRunning) r.pass("fail-badlab", `a bad lab folder is handled gracefully; nothing runs${refused ? "; the import said it failed" : ""}`);
  else {
    r.fail("fail-badlab", `usable: ${usable}, nothing running: ${noRunning}; main: ${main.slice(0, 200)}`);
  }
} catch (e) {
  r.fail("fail-badlab", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
