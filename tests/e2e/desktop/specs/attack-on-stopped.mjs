// Error/edge: mounting the attack-box panel against a lab that is NOT running must degrade
// gracefully — the panel renders a not-running / error state, never claims a box is up, and the
// app stays usable. No lab is launched; a lab id that nothing is running under is handed to the
// real panel (WdioAttack), exercising useAttackBox / attack_box_status on a dead lab.
import { finish, forceVisible, goTo, holdsFor, openApp, results } from "../support/app.mjs";

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
  // Nothing says the panel's status poll and auto-attach have given up on the dead lab, so the
  // panel is watched over the time they take to: at no look may it claim a box, by its Stop
  // control (shown only for a running box) or an address.
  let t = "";
  const claimsNoBox = async () => {
    t = await text(b);
    return !/Stop attack box/.test(t) && !/\d{1,3}(?:\.\d{1,3}){3}/.test(t);
  };
  if (!(await holdsFor(claimsNoBox, 8000))) throw new Error(`the panel claims a box (its Stop control or an address) for a dead lab:\n${t.slice(0, 300)}`);

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
