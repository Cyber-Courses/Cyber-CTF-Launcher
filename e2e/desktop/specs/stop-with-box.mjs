// Failure that shipped in 1.5.3: stopping a container lab while its attack box was attached left
// the box's container running (it has no Compose project label, so `down` never saw it), and the
// Machine page kept listing the lab as running. Launch the local lab, attach the box through the
// real panel, then stop the lab through the `lab_stop` command (what the lab page and the
// Overview call) and check Docker has nothing of the lab left, box included.
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { finish, forceVisible, goTo, mainText, openApp, sleep, waitAwake, waitForLab } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}
const panel = (b) =>
  b
    .$("[data-wdio-attack]")
    .getText()
    .catch(() => "");
const labContainers = (id) => execSync(`docker ps -a --format '{{.Names}}' --filter name=^cyberctf-${id}`, { encoding: "utf8" }).split("\n").filter(Boolean);

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, 220000, "lab never ran");
  const main = await mainText(b);
  const raw = (main.match(/local-[a-z0-9_-]+/) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  // The real attack-box panel for that lab attaches the box; wait until the runner reports it up.
  await b.execute((v) => localStorage.setItem("__wdio_attack__", v), id);
  await b.refresh();
  await b.$("[data-wdio-attack]").waitForExist({ timeout: 25000 });
  await forceVisible(b);
  await waitAwake(b, async () => /Stop attack box/.test(await panel(b)), { timeout: 90000, interval: 3000, timeoutMsg: "the box never came up" });
  const withBox = labContainers(id);
  if (!withBox.some((n) => n.endsWith("-isoloom-access"))) throw new Error(`no access container among ${JSON.stringify(withBox)}`);
  console.log("box attached; containers:", JSON.stringify(withBox));

  // Stop the lab as the lab page does: the `lab_stop` command, its log on a Tauri channel.
  await b.execute((v) => {
    window.__stop = "pending";
    const internals = window.__TAURI_INTERNALS__;
    const channel = internals.transformCallback(() => {});
    internals.invoke("lab_stop", { id: v, runtime: "DOCKER", logs: `__CHANNEL__:${channel}` }).then(
      () => (window.__stop = "ok"),
      (e) => (window.__stop = "ERR " + e),
    );
  }, id);
  let result = "pending";
  for (let i = 0; i < 150 && result === "pending"; i++) {
    await sleep(1000);
    result = await b.execute(() => window.__stop).catch(() => "pending");
  }
  if (result !== "ok") throw new Error(`lab_stop: ${result}`);
  await sleep(2000);
  const left = labContainers(id);
  if (left.length === 0) console.log("STOP_WITH_BOX_OK stopping the lab removed its containers, attack box included");
  else {
    console.log(`STOP_WITH_BOX_FAIL containers left after stop: ${JSON.stringify(left)}`);
    rc = 1;
  }
} catch (e) {
  console.log("STOP_WITH_BOX_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
