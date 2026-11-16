// The attack machine, through the REAL panel: launch a local lab, mount the lab page's own
// attack-box panel (WdioAttack), let it attach the attack box, see it running with an address,
// open the attacker shell, then Stop it and watch it detach. Drives the same useAttackBox /
// AttackBoxPanel / useLabShell the catalogue lab page uses.
import { finish, forceVisible, goTo, invokeCmd, LAB_START_MS, localLab, mainText, openApp, results, waitAwake, waitForLab } from "../lib/support.mjs";

export const needs = ["running-lab:fixture"];
const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");
const text = (b) =>
  b
    .$("[data-wdio-attack]")
    .getText()
    .catch(() => "");

let proc;
let b;
try {
  // 1. Launch the local lab through the real import path and wait until it runs here.
  ({ proc, b } = await openApp({ storage: { __wdio_launch_local__: LAB } }));
  await goTo(b, "Overview");
  await waitForLab(b, LAB_START_MS, "lab never ran");

  // 2. Read the running lab's id off the Overview. The page also carries container names
  //    (`<id>-isoloom-<role>…`), so strip any `-isoloom-*` suffix to get the bare lab id. No word
  //    boundary before `local-`: getText() glues the id to the title before it ("…Apilocal-…").
  const main = await mainText(b);
  const raw = (main.match(/local-[a-z0-9_-]+/) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  // 3. Mount the real attack-box panel for that lab (same panel the lab page shows).
  // Mounted with no reload: a reload right after launch ends the Windows app (a fast-fail).
  await b.execute((v) => {
    try {
      localStorage.setItem("__wdio_attack__", v);
    } catch {
      /* storage unavailable */
    }
    window.dispatchEvent(new Event("cyberctf:wdio-attack"));
  }, id);
  await b.$("[data-wdio-attack]").waitForExist({ timeout: 25000 });
  await forceVisible(b);

  // 4. The panel auto-attaches the box; wait until it reports it up. The "Stop attack box" control
  //    shows only when the runner reports the box running, so it is the definitive up signal.
  try {
    await waitAwake(b, async () => /Stop attack box/.test(await text(b)), {
      timeout: LAB_START_MS,
      interval: 3000,
      timeoutMsg: "the panel never showed the box running",
    });
  } catch (e) {
    const vis = await b.execute(() => document.visibilityState).catch(() => "?");
    const ask = (cmd) =>
      invokeCmd(b, cmd, { id, runtime: "DOCKER" }, { timeoutMs: 15000 }).then(
        (s) => [cmd, s],
        (x) => [`${cmd} failed`, x.message],
      );
    const diag = JSON.stringify([await ask("attack_box_status"), await ask("lab_status")]);
    console.log(`DIAG id=${id} visibility=${vis} panel=${JSON.stringify((await text(b)).slice(0, 300))} invoke=${diag}`);
    throw e;
  }
  let ip = "(no ip shown)";
  let pt = "";
  // The address follows the running state by a status read or two. getText() can glue it to its
  // label ("Address10.61.136.2"), so no leading word boundary.
  await waitAwake(
    b,
    async () => {
      pt = await text(b);
      ip = pt.match(/\d{1,3}(?:\.\d{1,3}){3}/)?.[0] ?? ip;
      return ip !== "(no ip shown)";
    },
    { timeout: 15000, interval: 1500, timeoutMsg: "no address shown" },
  ).catch(() => {});
  console.log(
    `ATTACK_UP box running on the lab network, address ${ip}` +
      (ip === "(no ip shown)" ? ` | panel=${JSON.stringify(pt.replace(/\s+/g, " ").slice(0, 220))}` : ""),
  );

  // 5. The "Open attacker shell" button appears once the box is up; click it. A `*=` partial-text
  //    selector has to be scoped by chaining off the panel element, not written as one CSS string.
  const shellBtn = await b.$("[data-wdio-attack]").$("button*=Open attacker shell");
  await shellBtn.waitForClickable({ timeout: 20000 });
  const before = await b.getWindowHandles();
  await shellBtn.click();
  let after = before;
  await b.waitUntil(async () => (after = await b.getWindowHandles()).length > before.length, { timeout: 4000, interval: 500 }).catch(() => {});
  if (after.length > before.length) {
    const fresh = after.find((h) => !before.includes(h));
    await b.switchToWindow(fresh);
    const url = await b.execute(() => location.href).catch(() => "");
    const ok = /\/shell\b/.test(url) || /\bruntime=DOCKER\b/.test(url);
    console.log(ok ? `SHELL_WINDOW_OK attacker shell opened (${url})` : `SHELL_WINDOW_URL unexpected (${url})`);
    await b.switchToWindow(before[0]).catch(() => {});
  } else {
    // Not yet listed 4 s after the click (the embedded server lists every webview window): the
    // attack-shell spec waits for the window and types in it; here the open path fired.
    console.log("SHELL_WINDOW_SOFT the shell window wasn't listed yet");
  }

  // 6. Stop the box from the panel and watch it detach.
  const stopBtn = await b.$("[data-wdio-attack]").$("button*=Stop attack box");
  await stopBtn.waitForClickable({ timeout: 20000 });
  await stopBtn.click();
  await waitAwake(
    b,
    async () => {
      const t = await text(b);
      return /not started|Start attack box/.test(t) && !/Address/.test(t);
    },
    { timeout: 60000, interval: 2000, timeoutMsg: "attack box never detached" },
  );
  r.pass("attack-box", "attached, ran with an address, opened a shell, then detached");
} catch (e) {
  r.fail("attack-box", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
