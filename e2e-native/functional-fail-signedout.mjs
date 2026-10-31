// Failure/edge scenario: a signed-out user. The app must stay fully usable (local labs) and prompt
// to sign in for website-launched labs, without errors.
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
const PORT = 4445;
const BIN = new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT:String(PORT) }, stdio:["ignore","pipe","pipe"] });
for (let i=0;i<60;i++){ try{ if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; }catch{} await sleep(1000); }
const b = await remote({ hostname:"127.0.0.1", port:PORT, path:"/", logLevel:"error", capabilities:{ browserName:"wry","wdio:enforceWebDriverClassic":true } });
let rc = 0;
try {
  await b.execute(() => { try { localStorage.setItem("cyberctf.onboarded","1"); localStorage.setItem("cyberctf.locale","en"); } catch(e){} });
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await (await b.$("aside nav").$("button*=Overview")).click();
  await sleep(4000);
  const body = (await b.$("body").getText().catch(()=>""));
  const signInPrompt = /sign in/i.test(body);
  const usable = await b.$("main h1").isExisting();
  if (signInPrompt && usable) console.log("FAIL_SIGNEDOUT_OK signed-out app is usable and prompts to sign in");
  else { console.log("FAIL_SIGNEDOUT_FAIL signIn=",signInPrompt,"usable=",usable); rc=1; }
} catch (e) { console.log("FAIL_SIGNEDOUT_ERR", e.message); rc=1; }
finally { await b.deleteSession().catch(()=>{}); try{proc.kill("SIGKILL");}catch{} }
process.exit(rc);
