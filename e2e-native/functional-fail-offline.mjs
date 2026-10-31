// Failure scenario: the Cyber CTF backend is unreachable. Point CYBERBACKEND_URL at a dead host;
// the catalogue must degrade gracefully (no crash) and the UI must stay usable with its machine panel.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
const PORT = 4445;
const BIN = new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT:String(PORT), CYBERBACKEND_URL: "http://127.0.0.1:1" }, stdio:["ignore","pipe","pipe"] });
for (let i=0;i<60;i++){ try{ if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; }catch{} await sleep(1000); }
const b = await remote({ hostname:"127.0.0.1", port:PORT, path:"/", logLevel:"error", capabilities:{ browserName:"wry","wdio:enforceWebDriverClassic":true } });
let rc = 0;
try {
  await b.execute(() => { try { localStorage.setItem("cyberctf.onboarded","1"); localStorage.setItem("cyberctf.locale","en"); } catch(e){} });
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await (await b.$("aside nav").$("button*=Labs")).click();
  await sleep(10000);
  const main = (await b.$("main").getText().catch(()=>""));
  console.log("labs lines:", JSON.stringify(main.split("\n").filter(l=>/reach|offline|sign in|couldn|labs|catalog/i.test(l)).slice(0,8)));
  const rendered = await b.$("aside nav").isExisting(); // app still usable, not crashed
  const degraded = /can.?t be reached|couldn|sign in|offline|no labs|loading labs/i.test(main);
  if (rendered && degraded) console.log("FAIL_OFFLINE_OK the app stays usable and shows the catalogue degraded");
  else { console.log("FAIL_OFFLINE_FAIL rendered=",rendered,"degraded=",degraded); rc=1; }
} catch (e) { console.log("FAIL_OFFLINE_ERR", e.message); rc=1; }
finally { await b.deleteSession().catch(()=>{}); try{proc.kill("SIGKILL");}catch{} }
process.exit(rc);
