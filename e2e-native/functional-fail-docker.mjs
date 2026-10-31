// Failure scenario: Docker engine is down. Point the app at a dead Docker socket; the UI must show
// the engine as not running and no lab running (the seed's launch cannot start).
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB, PORT = 4445;
const BIN = new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT:String(PORT), CYBERCTF_WDIO_SEED_LAB: LAB, DOCKER_HOST: "unix:///nonexistent/docker.sock" }, stdio:["ignore","pipe","pipe"] });
for (let i=0;i<60;i++){ try{ if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; }catch{} await sleep(1000); }
const b = await remote({ hostname:"127.0.0.1", port:PORT, path:"/", logLevel:"error", capabilities:{ browserName:"wry","wdio:enforceWebDriverClassic":true } });
let rc = 0;
try {
  await b.execute(() => { try { localStorage.setItem("cyberctf.onboarded","1"); localStorage.setItem("cyberctf.locale","en"); } catch(e){} });
  await b.refresh();
  await b.$("aside nav").waitForExist({ timeout: 25000 });
  await (await b.$("aside nav").$("button*=Overview")).click();
  await sleep(8000); // let the machine/docker checks run
  const main = (await b.$("main").getText().catch(()=>""));
  console.log("overview lines:", JSON.stringify(main.split("\n").filter(l=>/docker|engine|running|nothing|stopped/i.test(l)).slice(0,10)));
  const dockerDown = /engine.*(stop|not running|off)|docker.*(stop|not running|off|unavailable)/i.test(main) || /start the engine|engine is off|engine stopped/i.test(main);
  const noLab = /Nothing running/i.test(main) || !/lab running on this machine/i.test(main);
  console.log("dockerDown:", dockerDown, "noLabRunning:", noLab);
  if (dockerDown && noLab) console.log("FAIL_DOCKER_OK the UI reflects Docker engine down + no lab running");
  else { console.log("FAIL_DOCKER_FAIL did not clearly reflect Docker down"); rc = 1; }
} catch (e) { console.log("FAIL_DOCKER_ERR", e.message); rc = 1; }
finally { await b.deleteSession().catch(()=>{}); try{proc.kill("SIGKILL");}catch{} }
process.exit(rc);
