// Failure scenario: launch a bad lab folder (not a lab) -> the import shows "Couldn't import that lab".
import { remote } from "webdriverio"; import { spawn } from "node:child_process"; import { setTimeout as sleep } from "node:timers/promises";
const PORT=4445, BIN=new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc=spawn(BIN,[],{env:{...process.env,TAURI_WEBDRIVER_PORT:String(PORT)},stdio:["ignore","pipe","pipe"]});
for(let i=0;i<60;i++){try{if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok)break;}catch{}await sleep(1000);}
const b=await remote({hostname:"127.0.0.1",port:PORT,path:"/",logLevel:"error",capabilities:{browserName:"wry","wdio:enforceWebDriverClassic":true}});
let rc=0;
try{
  await b.execute(()=>{try{localStorage.setItem("cyberctf.onboarded","1");localStorage.setItem("cyberctf.locale","en");localStorage.setItem("__wdio_launch_local__","/tmp/not-a-lab-folder-xyz");}catch(e){}});
  await b.refresh();
  await b.$("aside nav").waitForExist({timeout:25000});
  await b.waitUntil(async()=>/couldn.?t import that lab/i.test(await b.$("body").getText().catch(()=>"")),{timeout:30000,interval:1000,timeoutMsg:"no import-failed toast"});
  console.log("FAIL_BADLAB_OK a bad lab folder shows the import-failed error");
}catch(e){console.log("FAIL_BADLAB_FAIL",e.message);try{console.log("body:",(await b.$("body").getText()).slice(-300));}catch{};rc=1;}
finally{await b.deleteSession().catch(()=>{});try{proc.kill("SIGKILL");}catch{}}
process.exit(rc);
