// Resilience: killing the app mid-deploy must not corrupt it; reopening renders a coherent state.
import { remote } from "webdriverio"; import { spawn } from "node:child_process"; import { setTimeout as sleep } from "node:timers/promises";
const LAB=process.env.CYBERCTF_TEST_LOCAL_LAB, PORT=4445, BIN=new URL("../../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
async function launch(seed){ const env={...process.env,TAURI_WEBDRIVER_PORT:String(PORT)}; if(seed) env.CYBERCTF_WDIO_SEED_LAB=LAB; const p=spawn(BIN,[],{env,stdio:["ignore","pipe","pipe"]}); for(let i=0;i<60;i++){try{if((await fetch(`http://127.0.0.1:${PORT}/status`)).ok)break;}catch{}await sleep(1000);} return p; }
async function connect(){ const b=await remote({hostname:"127.0.0.1",port:PORT,path:"/",logLevel:"error",capabilities:{browserName:"wry","wdio:enforceWebDriverClassic":true}}); await b.execute(()=>{try{localStorage.setItem("cyberctf.onboarded","1");localStorage.setItem("cyberctf.locale","en");}catch(e){}}); await b.refresh(); await b.$("aside nav").waitForExist({timeout:25000}); return b; }
let rc=0, p1, p2, b2;
try{
  p1=await launch(true); const b1=await connect(); await sleep(10000); // kill mid-deploy
  console.log("killing app mid-deploy");
  await b1.deleteSession().catch(()=>{}); try{p1.kill("SIGKILL");}catch{} p1=null; await sleep(4000);
  p2=await launch(false); b2=await connect();
  await (await b2.$("aside nav").$("button*=Overview")).click(); await sleep(6000);
  const h1 = await b2.$("main h1").getText().catch(()=>"");
  const coherent = h1.length>0 && await b2.$("aside nav").isExisting(); // rendered, not crashed
  const main = await b2.$("main").getText().catch(()=>"");
  const noError = !/something went wrong|uncaught|crash/i.test(main);
  if (coherent && noError) console.log("KILL_MIDDEPLOY_OK reopened coherently after a mid-deploy kill");
  else { console.log("KILL_MIDDEPLOY_FAIL coherent=",coherent,"noError=",noError); rc=1; }
}catch(e){console.log("KILL_MIDDEPLOY_FAIL",e.message);rc=1;}
finally{ if(b2) await b2.deleteSession().catch(()=>{}); for(const p of [p2,p1]){try{p&&p.kill("SIGKILL");}catch{}} }
process.exit(rc);
