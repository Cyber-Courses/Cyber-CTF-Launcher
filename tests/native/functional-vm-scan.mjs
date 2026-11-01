// The running-labs scan sees a VM lab, on UTM, libvirt or Hyper-V. Prerequisite (set up by the
// caller, run-all.sh does it for UTM on a Mac): an installed lab whose id is CYBERCTF_EXPECT_LAB,
// and a VM for it running under the hypervisor, named as the engine names it: `<name> · <machine>`
// for a UTM VM, `<name>_<machine>` for a libvirt domain (on qemu:///system) or `<name>-<machine>` for
// a Hyper-V VM, where `<name>` is the lab's isoloom name. Drives the real `running_labs` command, so
// it exercises the actual discover scan: the utmctl/virsh/Get-VM probe and the name matcher.
// Proven green on jarvis (libvirt), karen (Hyper-V) and a Mac with UTM.
import { finish, launchApp, connect, sleep } from "./lib.mjs";

const WANT = process.env.CYBERCTF_EXPECT_LAB || "solo-web";

let rc = 1;
let proc;
let b;
try {
  proc = await launchApp();
  b = await connect();
  // The scan is debounced and each hypervisor probe waits out its own timeout, so poll a few times.
  for (let attempt = 0; attempt < 8 && rc !== 0; attempt++) {
    await b.execute(() => {
      window.__scan = "pending";
      const inv = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (!inv) {
        window.__scan = "no-invoke";
        return;
      }
      inv("running_labs").then(
        (r) => {
          window.__scan = JSON.stringify(r);
        },
        (e) => {
          window.__scan = "ERR " + e;
        },
      );
    });
    let raw = "pending";
    for (let i = 0; i < 20 && raw === "pending"; i++) {
      await sleep(1000);
      raw = await b.execute(() => window.__scan).catch(() => "pending");
    }
    console.log(`attempt ${attempt}: running_labs=${raw}`);
    let list = [];
    try {
      list = JSON.parse(raw);
    } catch {
      /* not a list yet */
    }
    if (Array.isArray(list) && list.includes(WANT)) rc = 0;
    else await sleep(3000);
  }
  console.log(rc === 0 ? `VM_SCAN_OK the running-labs scan detected the VM lab ${WANT}` : `VM_SCAN_FAIL ${WANT} not detected`);
} catch (e) {
  console.log("VM_SCAN_ERR", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
