// Failure that shipped in 1.5.3: stopping a container lab while its attack box was attached left
// the box's container running (it has no Compose project label, so `down` never saw it), and the
// Machine page kept listing the lab as running. Launch the local lab, attach the box through the
// real panel, then stop the lab through the `lab_stop` command (what the lab page and the
// Overview call) and check Docker has nothing of the lab left, box included.
import {
  boxDiagnosis,
  eventually,
  finish,
  forceVisible,
  goTo,
  invokeCmd,
  LAB_START_MS,
  labContainers,
  localLab,
  mainText,
  openApp,
  results,
  waitAwake,
  waitForLab,
} from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
const panel = (b) =>
  b
    .$("[data-wdio-attack]")
    .getText()
    .catch(() => "");
const containerNames = (id) => labContainers(id).map((c) => c.name);

let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, LAB_START_MS, "lab never ran");
  const main = await mainText(b);
  const raw = (main.match(/local-[a-z0-9_-]+/) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  // The real attack-box panel for that lab attaches the box; wait until the runner reports it up.
  // Mounted with no reload: a reload right after launch ends the Windows app (a fast-fail).
  await b.execute((v) => {
    localStorage.setItem("__wdio_attack__", v);
    window.dispatchEvent(new Event("cyberctf:wdio-attack"));
  }, id);
  await b.$("[data-wdio-attack]").waitForExist({ timeout: 25000 });
  await forceVisible(b);
  await waitAwake(b, async () => /Stop attack box/.test(await panel(b)), { timeout: LAB_START_MS, interval: 3000 }).catch(async () => {
    // Say what was there: the panel's words and the lab's containers (a box slow to pull or start
    // on a loaded or nested-virtualized machine reads differently from one that failed).
    // With what the app knows of it: the box's status, the runner's last operation and its log.
    throw new Error(
      `the box never came up; the panel says ${JSON.stringify(await panel(b))}; the lab's containers: ${JSON.stringify(labContainers(id))}; ${await boxDiagnosis(b, id)}`,
    );
  });
  const withBox = containerNames(id);
  // The box is a machine of the lab: its `attackbox` service in the lab's own project.
  if (!withBox.some((n) => /-attackbox-\d+$/.test(n))) throw new Error(`no attack box container among ${JSON.stringify(withBox)}`);
  console.log("box attached; containers:", JSON.stringify(withBox));

  // Stop the lab as the lab page does: the `lab_stop` command, its log on a Tauri channel.
  await invokeCmd(b, "lab_stop", { id, runtime: "DOCKER" }, { streaming: true, timeoutMs: 150000 });
  // Compose has returned; Docker lists the removed containers a moment longer.
  let left = containerNames(id);
  await eventually(() => (left = containerNames(id)).length === 0, 30000, 1000);
  if (left.length === 0) r.pass("stop-with-box", "stopping the lab removed its containers, attack box included");
  else {
    r.fail("stop-with-box", `containers left after stop: ${JSON.stringify(left)}`);
  }
} catch (e) {
  r.fail("stop-with-box", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
