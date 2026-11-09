// Error/edge: mounting the attack-box panel against a lab that is NOT running must degrade
// gracefully — the panel renders a not-running / error state, never claims a box is up, and the
// app stays usable. No lab is launched; a lab id that nothing is running under is handed to the
// real panel (WdioAttack), exercising useAttackBox / attack_box_status on a dead lab.
import { finish, forceVisible, goTo, openApp, sleep } from "../support.mjs";

const DEAD_ID = "local-no-such-lab";
const text = (b) =>
  b
    .$("[data-wdio-attack]")
    .getText()
    .catch(() => "");

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: { __wdio_attack__: DEAD_ID } }));
  await b.$("[data-wdio-attack]").waitForExist({ timeout: 25000, timeoutMsg: "attack panel never mounted" });
  await forceVisible(b);
  // Give the panel's status poll / auto-attach time to resolve against the dead lab.
  await sleep(8000);
  const t = await text(b);
  if (/Stop attack box/.test(t)) throw new Error(`panel claims a box is running for a dead lab:\n${t.slice(0, 300)}`);
  if (/\d{1,3}(?:\.\d{1,3}){3}/.test(t)) throw new Error(`panel shows a box address for a dead lab:\n${t.slice(0, 300)}`);

  // The shell is still usable after the panel settled on the error path.
  await goTo(b, "Machine");
  await goTo(b, "Overview");
  if (!(await b.$("aside nav").isExisting())) throw new Error("shell unusable after the attack-panel error");
  console.log("ATTACK_ON_STOPPED_OK panel degraded gracefully, app still usable; panel=", JSON.stringify(t.replace(/\s+/g, " ").slice(0, 160)));
} catch (e) {
  console.log("ATTACK_ON_STOPPED_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
