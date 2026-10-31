// Functional GUI e2e: a running lab survives an app restart. Launch with the seed, verify running,
// kill the app, reopen WITHOUT the seed, and assert the lab is rediscovered as running.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { existsSync } from "node:fs";
const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);
const BIN = process.env.APP_BIN || new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
if (!LAB || !existsSync(LAB)) { console.log("no lab"); process.exit(2); }
async function launchApp(seed) {
  const env = { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT) };
  if (seed) env.CYBERCTF_WDIO_SEED_LAB = LAB;
  const proc = spawn(BIN, [], { env, stdio: ["ignore","pipe","pipe"] });
  for (let i=0;i<60;i++){ try{ if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; }catch{} await sleep(1000); }
  return proc;
}
async function connect() {
  const b = await remote({ hostname:"127.0.0.1", port:PORT, path:"/", logLevel:"error", capabilities:{ browserName:"wry","wdio:enforceWebDriverClassic":true } });
  await b.execute(() => { try { localStorage.setItem("cyberctf.onboarded","1"); localStorage.setItem("cyberctf.locale","en"); } catch(e){} });
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await (await b.$("aside nav").$("button*=Overview")).click();
  return b;
}
const running = async (b, timeout) => b.waitUntil(async () => {
  const t = await b.$("main").getText().catch(()=>""); return /\blab[s]? running on this machine\b/i.test(t) && !/Nothing running/i.test(t);
}, { timeout, interval: 4000, timeoutMsg: "lab not shown running" });
let rc = 0, app1, app2, b2;
try {
  app1 = await launchApp(true);
  const b1 = await connect();
  await running(b1, 220000);
  console.log("phase1: running after seed");
  await b1.deleteSession().catch(()=>{});
  try { app1.kill("SIGKILL"); } catch {} app1 = null;
  await sleep(4000);
  app2 = await launchApp(false);
  b2 = await connect();
  await running(b2, 70000);
  console.log("RESTART_PERSIST_OK lab still running after app restart");
} catch (e) {
  console.log("RESTART_PERSIST_FAIL", e.message);
  if (b2) { try { console.log("main:", JSON.stringify((await b2.$("main").getText()).slice(0,400))); } catch {} }
  rc = 1;
} finally {
  if (b2) await b2.deleteSession().catch(()=>{});
  for (const p of [app2, app1]) { try { p && p.kill("SIGKILL"); } catch {} }
}
process.exit(rc);
