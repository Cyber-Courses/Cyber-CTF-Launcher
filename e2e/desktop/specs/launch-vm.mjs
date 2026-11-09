// Functional GUI e2e: a catalogue container lab started "in a VM" on a local hypervisor through
// the REAL "Where should it run?" dialog, reaching running on its page, its published port
// answering, then stopped from the page and gone from the hypervisor. Needs a signed-in app (the
// lab comes from the catalogue, not an import) and a ready hypervisor with its Vagrant plugin.
//
//   CYBERCTF_TEST_VM_LAB       the lab's title in the Labs list (default: Supplier portal API)
//   CYBERCTF_TEST_VM_PROVIDER  the hypervisor to pick (default: utm on macOS)
//
// Exit 2 (skipped) when the lab isn't listed: not signed in, or not in this account's catalogue.
import { execFileSync } from "node:child_process";
import { finish, goTo, mainText, openApp, waitAwake } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_VM_LAB || "Supplier portal API";
const PROVIDER = process.env.CYBERCTF_TEST_VM_PROVIDER || (process.platform === "darwin" ? "utm" : "");
/** The hypervisor's label on the dialog's switch (src/features/machine/hypervisors.ts). */
const LABELS = { utm: "UTM", qemu: "QEMU", virtualbox: "VirtualBox", vmware_desktop: "VMware", parallels: "Parallels", libvirt: "libvirt", hyperv: "Hyper-V" };
const UTMCTL = "/Applications/UTM.app/Contents/MacOS/utmctl";
const say = (m) => console.log(`[${new Date().toISOString().slice(11, 19)}] ${m}`);

/** What the hypervisor holds (UTM: `utmctl list` without its header; others: unknown, empty). */
function vms() {
  if (PROVIDER !== "utm") return "";
  try {
    return execFileSync(UTMCTL, ["list"]).toString().trim().split("\n").slice(1).join("; ");
  } catch {
    return "";
  }
}
const pageText = (b) => mainText(b);
const mainButton = (b, ...labels) => b.$(`//main//button[${labels.map((l) => `normalize-space()=${JSON.stringify(l)}`).join(" or ")}]`);
/** Stop & remove from the page (the confirmation included), until the page offers Start again. */
async function stopAndRemove(b) {
  const stop = await mainButton(b, "Stop & remove", "Stop & clean up");
  await stop.waitForClickable({ timeout: 30000 });
  await stop.click();
  const confirm = await b.$('//*[@role="alertdialog"]//button[normalize-space()="Remove"]');
  if (await confirm.waitForClickable({ timeout: 10000 }).catch(() => false)) await confirm.click();
  await waitAwake(b, async () => await mainButton(b, "Start lab", "Start").isExisting(), { timeout: 300000, timeoutMsg: "the lab never stopped" });
}

if (!PROVIDER) {
  console.log("no hypervisor to pick (set CYBERCTF_TEST_VM_PROVIDER)");
  process.exit(2);
}

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Labs");
  await b.$("main").waitForExist({ timeout: 20000 });
  // The lab's card: the outermost clickable element in the list that carries its title.
  const card = await b.$(`(//main//*[@role="button" or @role="link" or self::a or self::button][contains(normalize-space(), ${JSON.stringify(LAB)})])[1]`);
  if (!(await card.waitForClickable({ timeout: 30000 }).catch(() => false))) {
    console.log(`LAUNCH_VM_SKIP the Labs list doesn't show "${LAB}" (signed out, or not in this catalogue)`);
    process.exit(2);
  }
  await card.click();
  await waitAwake(b, async () => /Stop & remove|Stop & clean up|Start lab|\bRetry\b|Sign in to start/.test(await pageText(b)), {
    timeout: 60000,
    timeoutMsg: "the lab page shows no action",
  });
  if (/Sign in to start/.test(await pageText(b))) {
    console.log("LAUNCH_VM_SKIP the app isn't signed in: a catalogue lab starts only once it is");
    process.exit(2);
  }
  say(`VMs before: ${vms() || "(none)"}`);
  if (/Stop & remove|Stop & clean up/.test(await pageText(b))) {
    say("the lab is up (or half up) from before: stopping it first");
    await stopAndRemove(b);
  }

  const start = await mainButton(b, "Start lab", "Start");
  await start.waitForClickable({ timeout: 60000 });
  await start.click();
  const dialog = await b.$('[role="dialog"]');
  await dialog.waitForExist({ timeout: 15000 });
  const inVm = await dialog.$('//*[@role="radio"][.//*[normalize-space()="This machine, in a VM"]]');
  if (!(await inVm.waitForClickable({ timeout: 10000 }).catch(() => false))) throw new Error(`the dialog offers no VM here:\n${await dialog.getText()}`);
  await inVm.click();
  // With several hypervisors ready the option carries a switch: pick the one asked for.
  const group = await dialog.$('[role="radiogroup"][aria-label="Hypervisor"]');
  if (await group.isExisting()) {
    const pick = await group.$(`//button[@role="radio"][normalize-space()=${JSON.stringify(LABELS[PROVIDER] ?? PROVIDER)}]`);
    if (!(await pick.isExisting())) throw new Error(`${PROVIDER} isn't on the switch: ${(await group.getText()).split("\n").join(" / ")}`);
    await pick.click();
  }
  const option = (await inVm.getText()).split("\n").join(" | ");
  if (!option.includes(LABELS[PROVIDER] ?? PROVIDER)) throw new Error(`the VM option doesn't name ${PROVIDER}: ${option}`);
  say(`picked: ${option}`);
  await (await dialog.$('//button[normalize-space()="Start"]')).click();

  const t0 = Date.now();
  await waitAwake(
    b,
    async () => {
      const t = await pageText(b);
      if (/Failed to start|Couldn.t start/.test(t)) throw new Error(`the start failed:\n${t.slice(0, 1500)}`);
      return /\bRunning\b.*on this machine/s.test(t) && /Stop & remove/.test(t);
    },
    { timeout: 900000, timeoutMsg: "the lab never showed as running" },
  );
  say(`running after ${Math.round((Date.now() - t0) / 1000)}s; VMs: ${vms() || "(none seen)"}`);
  if (PROVIDER === "utm" && !/started/.test(vms())) throw new Error(`no started UTM VM for the lab: ${vms()}`);
  const text = await pageText(b);
  const url = (text.match(/https?:\/\/127\.0\.0\.1:\d+\/?/) || [])[0];
  if (url) {
    const status = await fetch(url)
      .then((r) => r.status)
      .catch((e) => `error ${e.message}`);
    say(`${url} -> HTTP ${status}`);
    if (status !== 200) throw new Error(`the lab's page says ${url} but it answers ${status}`);
  } else {
    say("the page names no local URL (no published port): running state is the evidence");
  }

  await stopAndRemove(b);
  const left = vms();
  say(`VMs after stop: ${left || "(none)"}`);
  if (PROVIDER === "utm" && /started|stopped/.test(left)) throw new Error(`a VM is left in UTM: ${left}`);
  console.log("LAUNCH_VM_OK a container lab ran in a VM on " + PROVIDER + " through the dialog and left nothing behind");
} catch (e) {
  console.log("LAUNCH_VM_FAIL", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
