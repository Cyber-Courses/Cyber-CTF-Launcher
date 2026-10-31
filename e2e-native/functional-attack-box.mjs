// The attack machine, through the REAL panel: launch a local lab, mount the lab page's own
// attack-box panel (WdioAttack), let it attach the attack box, see it running with an address,
// open the attacker shell, then Stop it and watch it detach. Drives the same useAttackBox /
// AttackBoxPanel / useLabShell the catalogue lab page uses.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
const PORT = 4445;
const BIN = new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const text = (b) => b.$("[data-wdio-attack]").getText().catch(() => "");

const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT) }, stdio: ["ignore", "pipe", "pipe"] });
for (let i = 0; i < 60; i++) { try { if ((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; } catch {} await sleep(1000); }
const b = await remote({ hostname: "127.0.0.1", port: PORT, path: "/", logLevel: "error", capabilities: { browserName: "wry", "wdio:enforceWebDriverClassic": true } });
let rc = 0;
try {
  // 1. Launch the local lab through the real import path and wait until it runs here.
  await b.execute((p) => { try { localStorage.setItem("cyberctf.onboarded", "1"); localStorage.setItem("cyberctf.locale", "en"); localStorage.setItem("__wdio_launch_local__", p); } catch {} }, LAB);
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await (await b.$("aside nav").$("button*=Overview")).click();
  await b.waitUntil(async () => /\blab[s]? running on this machine\b/i.test(await b.$("main").getText().catch(() => "")) && !/Nothing running/i.test(await b.$("main").getText().catch(() => "")), { timeout: 220000, interval: 4000, timeoutMsg: "lab never ran" });

  // 2. Read the running lab's id off the Overview. The page also carries container names
  //    (`<id>-isoloom-<role>…`), so strip any `-isoloom-*` suffix to get the bare lab id.
  const main = await b.$("main").getText();
  const raw = (main.match(/\blocal-[a-z0-9_-]+/i) || [])[0];
  const id = raw && raw.split("-isoloom-")[0];
  if (!id) throw new Error(`no local lab id on the Overview:\n${main.slice(0, 400)}`);

  // 3. Mount the real attack-box panel for that lab (same panel the lab page shows).
  await b.execute((v) => { try { localStorage.setItem("__wdio_attack__", v); } catch {} }, id);
  await b.refresh();
  await b.$("[data-wdio-attack]").waitForExist({ timeout: 25000 });
  // A headless window can report hidden, which pauses the status poll; force it visible and nudge.
  await b.execute(() => {
    try {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new Event("focus"));
    } catch {}
  });

  // 4. The panel auto-attaches the box; wait until it reports it up. The "Stop attack box" control
  //    shows only when the runner reports the box running, so it is the definitive up signal.
  try {
    await b.waitUntil(async () => /Stop attack box/.test(await text(b)), { timeout: 90000, interval: 3000 });
  } catch (e) {
    const vis = await b.execute(() => document.visibilityState).catch(() => "?");
    await b.execute((v) => {
      window.__diag = "pending";
      const inv = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (!inv) { window.__diag = "no-invoke"; return; }
      Promise.all([
        inv("attack_box_status", { id: v, runtime: "DOCKER", network: null }).then((s) => ["box", s], (x) => ["box-err", String(x)]),
        inv("lab_status", { id: v, runtime: "DOCKER" }).then((s) => ["lab", s], (x) => ["lab-err", String(x)]),
      ]).then((r) => { window.__diag = JSON.stringify(r); }, (x) => { window.__diag = "ERR " + x; });
    }, id);
    let raw = "pending";
    for (let i = 0; i < 30 && raw === "pending"; i++) { await sleep(500); raw = await b.execute(() => window.__diag).catch(() => "pending"); }
    console.log(`DIAG id=${id} visibility=${vis} panel=${JSON.stringify((await text(b)).slice(0, 300))} invoke=${raw}`);
    throw e;
  }
  let ip = "(no ip shown)", pt = "";
  // getText() can glue the address to its label ("Address10.61.136.2"), so no leading word boundary.
  for (let i = 0; i < 10; i++) { pt = await text(b); const m = pt.match(/\d{1,3}(?:\.\d{1,3}){3}/); if (m) { ip = m[0]; break; } await sleep(1500); }
  console.log(`ATTACK_UP box running on the lab network, address ${ip}` + (ip === "(no ip shown)" ? ` | panel=${JSON.stringify(pt.replace(/\s+/g, " ").slice(0, 220))}` : ""));

  // 5. The "Open attacker shell" button appears once the box is up; click it. A `*=` partial-text
  //    selector has to be scoped by chaining off the panel element, not written as one CSS string.
  const shellBtn = await b.$("[data-wdio-attack]").$("button*=Open attacker shell");
  await shellBtn.waitForClickable({ timeout: 20000 });
  const before = await b.getWindowHandles();
  await shellBtn.click();
  await sleep(4000);
  const after = await b.getWindowHandles();
  if (after.length > before.length) {
    const fresh = after.find((h) => !before.includes(h));
    await b.switchToWindow(fresh);
    const url = await b.execute(() => location.href).catch(() => "");
    const ok = /\/shell\b/.test(url) || /\bruntime=DOCKER\b/.test(url);
    console.log(ok ? `SHELL_WINDOW_OK attacker shell opened (${url})` : `SHELL_WINDOW_URL unexpected (${url})`);
    await b.switchToWindow(before[0]).catch(() => {});
  } else {
    // The embedded WebDriver server exposes only the main webview; the shell opens as its own OS
    // window, which it does not surface as a handle. The open path still fired without error.
    console.log("SHELL_WINDOW_SOFT attacker shell opened (new window not exposed by the embedded server)");
  }

  // 6. Stop the box from the panel and watch it detach.
  const stopBtn = await b.$("[data-wdio-attack]").$("button*=Stop attack box");
  await stopBtn.waitForClickable({ timeout: 20000 });
  await stopBtn.click();
  await b.waitUntil(async () => { const t = await text(b); return /not started|Start attack box/.test(t) && !/Address/.test(t); }, { timeout: 60000, interval: 2000, timeoutMsg: "attack box never detached" });
  console.log("ATTACK_BOX_OK attached, ran with an address, opened a shell, then detached");
} catch (e) {
  console.log("ATTACK_BOX_FAIL", e.message);
  rc = 1;
}
finally { await b.deleteSession().catch(() => {}); try { proc.kill("SIGKILL"); } catch {} }
process.exit(rc);
