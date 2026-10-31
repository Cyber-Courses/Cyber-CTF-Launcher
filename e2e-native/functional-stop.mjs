// Functional: stop a running lab from the Machine > Running now panel; it must stop and clear.
import { remote } from "webdriverio"; import { spawn } from "node:child_process"; import { setTimeout as sleep } from "node:timers/promises";
const LAB=process.env.CYBERCTF_TEST_LOCAL_LAB, PORT=4445, BIN=new URL("../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const proc=spawn(BIN,[],{env:{...process.env,TAURI_WEBDRIVER_PORT:String(PORT),CYBERCTF_WDIO_SEED_LAB:LAB},stdio:["ignore","pipe","pipe"]});
for(let i=0;i<60;i++){try{if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok)break;}catch{}await sleep(1000);}
const b=await remote({hostname:"127.0.0.1",port:PORT,path:"/",logLevel:"error",capabilities:{browserName:"wry","wdio:enforceWebDriverClassic":true}});
let rc=0;
try{
  await b.execute(()=>{try{localStorage.setItem("cyberctf.onboarded","1");localStorage.setItem("cyberctf.locale","en");}catch(e){}});
  await b.refresh();
  await b.$("aside nav").waitForExist({timeout:25000});
  await (await b.$("aside nav").$("button*=Overview")).click();
  await b.waitUntil(async()=>/\blab[s]? running on this machine\b/i.test(await b.$("main").getText().catch(()=>"")),{timeout:220000,interval:4000,timeoutMsg:"lab not running"});
  await (await b.$("aside nav").$("button*=Machine")).click();
  const stopBtn = await b.$("main").$("button=Stop");
  await stopBtn.waitForClickable({timeout:30000});
  console.log("clicking Stop on the running workload");
  await stopBtn.click();
  await (await b.$("aside nav").$("button*=Overview")).click();
  await b.waitUntil(async()=>/Nothing running/i.test(await b.$("main").getText().catch(()=>"")) || !/\blab[s]? running on this machine\b/i.test(await b.$("main").getText().catch(()=>"")),{timeout:60000,interval:3000,timeoutMsg:"lab did not stop"});
  console.log("STOP_OK the running lab stopped and cleared");
}catch(e){console.log("STOP_FAIL",e.message);rc=1;}
finally{await b.deleteSession().catch(()=>{});try{proc.kill("SIGKILL");}catch{}}
process.exit(rc);
