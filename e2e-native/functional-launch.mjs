// Functional GUI e2e: the app seeds a running lab on startup (CYBERCTF_WDIO_SEED_LAB, wdio build);
// this drives the REAL UI to verify the lab shows as running on the Overview.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { existsSync } from "node:fs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
const PORT = Number(process.env.TAURI_WEBDRIVER_PORT || 4445);
const BIN = process.env.APP_BIN || new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
if (!LAB || !existsSync(LAB)) { console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`); process.exit(2); }

const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT), CYBERCTF_WDIO_SEED_LAB: LAB }, stdio: ["ignore","pipe","pipe"] });
let out = ""; proc.stdout.on("data", d => out += d); proc.stderr.on("data", d => out += d);
for (let i=0;i<60;i++){ try{ if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; }catch{} await sleep(1000); }

const b = await remote({ hostname:"127.0.0.1", port:PORT, path:"/", logLevel:"error", capabilities:{ browserName:"wry","wdio:enforceWebDriverClassic":true } });
let rc = 0;
try {
  await b.execute(() => { try { localStorage.setItem("cyberctf.onboarded","1"); localStorage.setItem("cyberctf.locale","en"); } catch(e){} });
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await b.$("main h1").waitForExist({ timeout: 25000 });
  console.log("shell rendered; h1 =", JSON.stringify(await b.$("main h1").getText()));
  await (await b.$("aside nav").$("button*=Overview")).click();
  await b.waitUntil(async () => {
    const txt = await b.$("main").getText().catch(() => "");
    return /\blab[s]? running on this machine\b/i.test(txt) && !/Nothing running/i.test(txt);
  }, { timeout: 220000, interval: 4000, timeoutMsg: "the seeded lab never showed as running on Overview" });
  const running = (await b.$("main").getText()).split("\n").filter(l => /running/i.test(l)).slice(0,4);
  console.log("FUNCTIONAL_LAUNCH_OK running lines:", JSON.stringify(running));
} catch (e) {
  console.log("FUNCTIONAL_LAUNCH_FAIL", e.message);
  try { console.log("main text:", JSON.stringify((await b.$("main").getText()).slice(0,600))); } catch {}
  console.log("app out tail:", out.slice(-600));
  rc = 1;
} finally { await b.deleteSession().catch(()=>{}); try { proc.kill("SIGKILL"); } catch {} }
process.exit(rc);
