// Happy path through the REAL import launch (frontend-tracked): start a local lab, reach running.
import { remote } from "webdriverio"; import { spawn } from "node:child_process"; import { setTimeout as sleep } from "node:timers/promises";
const LAB=process.env.CYBERCTF_TEST_LOCAL_LAB, PORT=4445, BIN=new URL("../../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc=spawn(BIN,[],{env:{...process.env,TAURI_WEBDRIVER_PORT:String(PORT)},stdio:["ignore","pipe","pipe"]});
for(let i=0;i<60;i++){try{if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok)break;}catch{}await sleep(1000);}
const b=await remote({hostname:"127.0.0.1",port:PORT,path:"/",logLevel:"error",capabilities:{browserName:"wry","wdio:enforceWebDriverClassic":true}});
let rc=0;
try{
  await b.execute((p)=>{try{localStorage.setItem("cyberctf.onboarded","1");localStorage.setItem("cyberctf.locale","en");localStorage.setItem("__wdio_launch_local__",p);}catch(e){}},LAB);
  await b.refresh();
  await b.$("aside nav").waitForExist({timeout:25000});
  await (await b.$("aside nav").$("button*=Overview")).click();
  await b.waitUntil(async()=>/\blab[s]? running on this machine\b/i.test(await b.$("main").getText().catch(()=>""))&&!/Nothing running/i.test(await b.$("main").getText().catch(()=>"")),{timeout:220000,interval:4000,timeoutMsg:"lab not running"});
  console.log("LAUNCH_UI_OK the real import launch reached running");
}catch(e){console.log("LAUNCH_UI_FAIL",e.message);rc=1;}
finally{await b.deleteSession().catch(()=>{});try{proc.kill("SIGKILL");}catch{}}
process.exit(rc);
