// Failure scenario: a bad lab folder (not a lab) must be handled gracefully -- the app stays usable
// and nothing ends up running (no crash, no phantom lab).
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
  await (await b.$("aside nav").$("button*=Overview")).click();
  await sleep(10000); // let the bad launch attempt settle
  const usable = (await b.$("main h1").isExisting()) && (await b.$("aside nav").isExisting());
  const main = await b.$("main").getText().catch(()=>"");
  const noRunning = /Nothing running/i.test(main) || !/\blab[s]? running on this machine\b/i.test(main);
  if (usable && noRunning) console.log("FAIL_BADLAB_OK a bad lab folder is handled gracefully; nothing runs");
  else { console.log("FAIL_BADLAB_FAIL usable=",usable,"noRunning=",noRunning,"main=",JSON.stringify(main.slice(0,200))); rc=1; }
}catch(e){console.log("FAIL_BADLAB_FAIL",e.message);rc=1;}
finally{await b.deleteSession().catch(()=>{});try{proc.kill("SIGKILL");}catch{}}
process.exit(rc);
