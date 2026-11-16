// The running-labs scan sees a VM lab, on UTM, libvirt or Hyper-V. Prerequisite (set up by the
// caller: tests/desktop/run.sh does it, the `fixture-vm` need, with UTM on a Mac and libvirt on
// Linux; lib/fixture.mjs): an installed lab whose id is CYBERCTF_TEST_SCAN_LAB,
// and a VM for it running under the hypervisor, named as the engine names it: `<name> · <machine>`
// for a UTM VM, `<name>_<machine>` for a libvirt domain (on qemu:///system) or `<name>-<machine>` for
// a Hyper-V VM, where `<name>` is the lab's isoloom name. Drives the real `running_labs` command, so
// it exercises the actual discover scan: the utmctl/virsh/Get-VM probe and the name matcher.
// Docker is made unreachable for the app (deadDockerEnv), so only the hypervisor probe can report the
// lab: its containers may still be up from the scenarios before.
// Proven green on libvirt, Hyper-V and a Mac with UTM.
import { capMs, connect, deadDockerEnv, eventually, finish, invokeCmd, launchApp, results } from "../lib/support.mjs";

const WANT = process.env.CYBERCTF_TEST_SCAN_LAB || "solo-web";

export const needs = ["running-lab:fixture", "fixture-vm"];
const r = results();
let found = false;
let proc;
let b;
try {
  proc = await launchApp({ env: deadDockerEnv() });
  b = await connect();
  // The scan is debounced and each hypervisor probe waits out its own timeout, so it is asked
  // again until it lists the lab.
  let attempt = 0;
  found = await eventually(
    async () => {
      attempt += 1;
      const list = await invokeCmd(b, "running_labs", {}, { timeoutMs: 20000 }).catch((e) => `ERR ${e.message}`);
      console.log(`attempt ${attempt}: running_labs=${JSON.stringify(list)}`);
      return Array.isArray(list) && list.includes(WANT);
    },
    capMs(0.6),
    3000,
  );
  r.check("vm-scan", found, found ? `the running-labs scan detected the VM lab ${WANT}` : `${WANT} not detected`);
} catch (e) {
  r.fail("vm-scan", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
