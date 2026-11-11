// Error/edge: mounting the attack-box panel against a lab that is NOT running must degrade
// gracefully — the panel renders a not-running / error state, never claims a box is up, and the
// app stays usable. No lab is launched; a lab id that nothing is running under is handed to the
// real panel (WdioAttack), exercising useAttackBox / attack_box_status on a dead lab.
import { finish, forceVisible, goTo, openApp, results, sleep } from "../lib/support.mjs";

const DEAD_ID = "local-no-such-lab";
const text = (b) =>
  b
    .$("[data-wdio-attack]")
    .getText()
    .catch(() => "");

export const needs = [];
const r = results();
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
  r.pass("attack-on-stopped", `the panel degraded gracefully and the app stays usable; panel: ${t.replace(/\s+/g, " ").slice(0, 160)}`);
} catch (e) {
  r.fail("attack-on-stopped", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
