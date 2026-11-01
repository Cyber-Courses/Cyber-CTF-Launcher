// The running-labs scan sees a VM lab, on libvirt or Hyper-V. Prerequisite (set up by the caller):
// an installed lab whose isoloom name is CYBERCTF_EXPECT_LAB, and a VM for it running under the
// hypervisor, named as the engine names it: `<name>_<machine>` for a libvirt domain (on
// qemu:///system) or `<name>-<machine>` for a Hyper-V VM. Drives the real `running_labs` command,
// so it exercises the actual discover scan: the virsh/Get-VM probe and the `<lab><sep><machine>`
// matcher. Proven green on jarvis (libvirt) and karen (Hyper-V).
import { remote } from "webdriverio";
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = 4445;
// CYBERCTF_BIN overrides the path (needed on Windows, where the binary is cyberctf-desktop.exe).
const BIN = process.env.CYBERCTF_BIN || new URL("../../src-tauri/target/debug/cyberctf-desktop", import.meta.url).pathname;
const WANT = process.env.CYBERCTF_EXPECT_LAB || "solo-web";

const proc = spawn(BIN, [], { env: { ...process.env, TAURI_WEBDRIVER_PORT: String(PORT) }, stdio: ["ignore", "pipe", "pipe"] });
for (let i = 0; i < 60; i++) { try { if ((await fetch(`http://127.0.0.1:${PORT}/status`)).ok) break; } catch {} await sleep(1000); }
const b = await remote({ hostname: "127.0.0.1", port: PORT, path: "/", logLevel: "error", capabilities: { browserName: "wry", "wdio:enforceWebDriverClassic": true } });
let rc = 1;
try {
  // The scan is debounced and each hypervisor probe waits out its own timeout, so poll a few times.
  for (let attempt = 0; attempt < 8 && rc !== 0; attempt++) {
    await b.execute(() => {
      window.__scan = "pending";
      const inv = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (!inv) { window.__scan = "no-invoke"; return; }
      inv("running_labs").then((r) => { window.__scan = JSON.stringify(r); }, (e) => { window.__scan = "ERR " + e; });
    });
    let raw = "pending";
    for (let i = 0; i < 20 && raw === "pending"; i++) { await sleep(1000); raw = await b.execute(() => window.__scan).catch(() => "pending"); }
    console.log(`attempt ${attempt}: running_labs=${raw}`);
    let list = [];
    try { list = JSON.parse(raw); } catch {}
    if (Array.isArray(list) && list.includes(WANT)) rc = 0;
    else await sleep(3000);
  }
  console.log(rc === 0 ? `VM_SCAN_OK the running-labs scan detected the VM lab ${WANT}` : `VM_SCAN_FAIL ${WANT} not detected`);
} catch (e) {
  console.log("VM_SCAN_ERR", e.message);
  rc = 1;
}
finally { await b.deleteSession().catch(() => {}); try { proc.kill("SIGKILL"); } catch {} }
process.exit(rc);
