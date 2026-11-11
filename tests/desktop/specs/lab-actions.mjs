// A running lab's actions on its page, on a catalogue container lab: Pause and Resume (and Shut
// down when the lab offers it), Reset (one of its containers stopped behind its back: the page
// says it is down and offers Reset lab, which starts the lab again clean), and Stop & remove, whose
// system confirmation (`confirmSystemDialogs`) is first answered Cancel (the lab keeps running),
// then Remove (nothing of it left). Each checked in Docker as well as on the page. The same
// actions on other forms are the integration tests' (park and resume per form); the commands
// under them are park-resume's and stop-with-box's.
//
//   CYBERCTF_TEST_CATALOGUE_LAB  the catalogue container lab (default Supplier portal API)
// Needs the app signed in and a Docker engine.
import {
  CATALOGUE_LAB,
  confirmSystemDialogs,
  docker,
  dockerUp,
  finish,
  labPageRunning,
  mainButton,
  mainText,
  openApp,
  openLabFromList,
  results,
  runContainers,
  signedIn,
  sleep,
  startFromDialog,
  stopAndRemoveOnPage,
  stopLeftover,
  waitAwake,
  waitLabPageRunning,
} from "../lib/support.mjs";
import { runRoot } from "../lib/owned.mjs";

const r = results();
if (!dockerUp()) r.skipAll("no Docker engine answers here");

/** The lab containers here (`cyberctf-*`), by name, with their state. */
function containers() {
  const out = docker("ps", "-a", "--format", "{{.Names}}\t{{.State}}", "--filter", "name=^cyberctf-") ?? "";
  return new Map(
    out
      .split("\n")
      .filter(Boolean)
      .map((l) => l.split("\t")),
  );
}
/** This lab's containers: the ones that weren't there before it started. */
const mine = (before) => [...containers()].filter(([name]) => !before.has(name));
const runningOf = (list) => list.filter(([, state]) => state === "running").map(([name]) => name);

/** Clicks the page's `label` button and waits until `done()`. */
async function act(b, label, done, timeout = 300000) {
  const button = await mainButton(b, label);
  await button.waitForClickable({ timeout: 60000 });
  await button.click();
  await waitAwake(b, done, { timeout, interval: 3000, timeoutMsg: `${label} never finished` });
}

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  if (!(await signedIn(b))) r.skipAll("the app isn't signed in: a catalogue lab starts only signed in (give CYBERCTF_TEST_ACCOUNT_SESSION to the runner)");
  if (!(await openLabFromList(b, CATALOGUE_LAB))) r.skipAll(`the catalogue doesn't list "${CATALOGUE_LAB}"`);
  await stopLeftover(b, r);
  const before = containers();
  await startFromDialog(b, { place: "container" });
  await waitLabPageRunning(b, 1200000);
  const up = runningOf(mine(before));
  if (up.length === 0) throw new Error("the page says the lab runs, but none of its containers does");

  if (await (await mainButton(b, "Pause")).isExisting()) {
    await r.step("pause", "Pause parks the lab: nothing of it runs, the page offers Resume", async () => {
      await act(b, "Pause", async () => (await mainButton(b, "Resume").isExisting()) && !/Pausing/.test(await mainText(b)));
      const still = runningOf(mine(before));
      if (still.length) throw new Error(`still running: ${still.join(", ")}`);
    });
    await r.step("resume", "Resume brings it back as it was", async () => {
      await act(b, "Resume", () => labPageRunning(b));
      const back = runningOf(mine(before));
      const missing = up.filter((n) => !back.includes(n));
      if (missing.length) throw new Error(`not running again: ${missing.join(", ")}`);
    });
  } else {
    r.skip("pause", "the page offers no Pause for this lab");
    r.skip("resume", "the page offers no Pause for this lab");
  }

  // Reset: a machine down behind the lab's back, as a crash would leave it; a container of the
  // run's own Compose project, never one by its name alone.
  const own = runContainers(runRoot());
  const victim = up.find((n) => !/attackbox/.test(n) && own.includes(n));
  if (victim) {
    docker("stop", victim);
    await r.step("down", `the page says ${victim} is down and offers Reset lab`, () =>
      waitAwake(b, async () => /is down|are down/.test(await mainText(b)) && (await mainButton(b, "Reset lab").isExisting()), {
        timeout: 180000,
        interval: 3000,
        timeoutMsg: "no down banner with Reset lab",
      }),
    );
    await r.step("reset", "Reset lab removes the lab and starts it again: every machine runs", async () => {
      await (await mainButton(b, "Reset lab")).click();
      await sleep(5000);
      await waitAwake(b, async () => !/Resetting/.test(await mainText(b)) && (await labPageRunning(b)), {
        timeout: 1200000,
        interval: 5000,
        timeoutMsg: "the lab never ran again after Reset",
      });
      const back = runningOf(mine(before));
      if (!back.some((n) => n === victim || n.replace(/-\d+$/, "") === victim.replace(/-\d+$/, "")))
        throw new Error(`${victim} isn't running again: ${back.join(", ")}`);
      if (/is down|are down/.test(await mainText(b))) throw new Error("the page still says a machine is down");
    });
  } else {
    r.skip("down", "no container of the lab to stop");
    r.skip("reset", "no container of the lab to stop");
  }

  await r.step("remove-cancel", "Stop & remove, answered Cancel in the system dialog, leaves the lab running", async () => {
    await confirmSystemDialogs(b, "cancel");
    await (await mainButton(b, "Stop & remove")).click();
    await sleep(5000);
    if (!(await labPageRunning(b))) throw new Error("the lab no longer shows running after a Cancel");
    if (runningOf(mine(before)).length === 0) throw new Error("its containers stopped after a Cancel");
  });
  await r.step("remove", "Stop & remove, confirmed, leaves nothing of the lab", async () => {
    await stopAndRemoveOnPage(b);
    await sleep(3000);
    const left = mine(before).map(([n]) => n);
    if (left.length) throw new Error(`left: ${left.join(", ")}`);
  });
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
