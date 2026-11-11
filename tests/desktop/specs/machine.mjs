// The Machine page and the guided setup. The page: what this computer can run (Providers), its
// load (CPU, Memory, Disk), what it runs now and its downloads; a provider's Test runs the real
// self-test (the container lab test: two containers on a lab network, then removed; the VM lab
// test with CYBERCTF_TEST_VM_PROVIDER, which boots a real VM). Its "Add a provider" opens the
// setup window (`machine-setup`). The guided setup itself opens from the Overview when Docker
// doesn't answer (the app given a dead Docker, as fail-docker does): its first step, "What do you
// want to run?", leads into a path. Nothing is installed: installing every tool on a bare machine
// is the integration tests' bare-machine tier (docs/testing/integration.md).
//
//   CYBERCTF_TEST_VM_PROVIDER  the hypervisor whose VM lab test to run (none: that check skipped)
import { closeWindow, deadDockerEnv, dockerUp, finish, goTo, mainText, openApp, results, switchToWindow, waitAwake } from "../lib/support.mjs";

const r = results();
const PROVIDER = process.env.CYBERCTF_TEST_VM_PROVIDER || "";
const LABELS = { utm: "UTM", qemu: "QEMU", virtualbox: "VirtualBox", vmware_desktop: "VMware", parallels: "Parallels", libvirt: "libvirt", hyperv: "Hyper-V" };

/** The self-test panel titled `title` once it says Passed or Failed (null while it runs). */
async function testOutcome(b, title) {
  const panel = await b.$(
    `//main//*[normalize-space()=${JSON.stringify(title)}]/ancestor::div[.//*[normalize-space()="Passed" or normalize-space()="Failed" or normalize-space()="Testing"]][1]`,
  );
  if (!(await panel.isExisting())) return null;
  const text = await panel.getText();
  if (/\bFailed\b/.test(text)) return { ok: false, text };
  if (/\bPassed\b/.test(text) && !/\bTesting\b/.test(text)) return { ok: true, text };
  return null;
}

/** Clicks `button` and waits for the self-test `title` to end; its outcome. */
async function runSelfTest(b, button, title, timeout) {
  await button.waitForClickable({ timeout: 20000 });
  await button.click();
  let outcome = null;
  await waitAwake(b, async () => (outcome = await testOutcome(b, title)) !== null, { timeout, interval: 5000, timeoutMsg: `the ${title} never ended` });
  return outcome;
}

let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Machine");
  await r.step("page", "the Machine page shows the providers, the load, what runs and the downloads", async () => {
    await b.waitUntil(async () => /Providers/.test(await mainText(b)), { timeout: 60000, timeoutMsg: "no Providers panel (the machine check never answered)" });
    const text = await mainText(b);
    const missing = ["Providers", "CPU", "Memory", "Disk", "Running now", "Downloads"].filter((w) => !text.includes(w));
    if (missing.length) throw new Error(`missing: ${missing.join(", ")}`);
    return `providers listed: ${/Ready for labs/.test(text) ? "ready for labs" : (text.match(/\d+ lab types? needs? setup/)?.[0] ?? "status unread")}`;
  });

  if (dockerUp()) {
    const outcome = await runSelfTest(b, await b.$('(//main//button[normalize-space()="Test"])[1]'), "Container lab test", 600000).catch((e) => ({
      ok: false,
      text: e.message,
    }));
    r.check("container-test", outcome.ok, `the container lab test from the engine's row: ${outcome.text.replace(/\s+/g, " ").slice(0, 300)}`);
  } else r.skip("container-test", "no Docker engine answers here");

  if (PROVIDER) {
    const label = LABELS[PROVIDER] ?? PROVIDER;
    const test = await b.$(
      `(//main//*[contains(normalize-space(text()), ${JSON.stringify(label)})]/ancestor::div[.//button[normalize-space()="Test"]][1]//button[normalize-space()="Test"])[1]`,
    );
    if (await test.isExisting()) {
      const outcome = await runSelfTest(b, test, "VM lab test", 1800000).catch((e) => ({ ok: false, text: e.message }));
      r.check("vm-test", outcome.ok, `the VM lab test on ${label}: ${outcome.text.replace(/\s+/g, " ").slice(0, 300)}`);
    } else r.fail("vm-test", `no ${label} row with a Test button on the Machine page`);
  } else r.skip("vm-test", "no hypervisor asked: set CYBERCTF_TEST_VM_PROVIDER");

  await (await b.$('//main//button[contains(normalize-space(), "Add a provider")]')).click();
  await r.step("add-provider", "Add a provider opens the setup window on what this machine can add", async () => {
    await switchToWindow(b, "machine-setup");
    await b.waitUntil(async () => /Add a provider/i.test(await b.$("body").getText()), { timeout: 30000, timeoutMsg: "the window doesn't say Add a provider" });
  });
  await closeWindow(b, "machine-setup");
  await finish(b, proc);
  b = null;
  proc = null;

  // The guided setup, from the Overview of a machine where Docker doesn't answer.
  ({ proc, b } = await openApp({ env: deadDockerEnv() }));
  await goTo(b, "Overview");
  const setUp = await b.$('//main//button[contains(normalize-space(), "Set up this machine")]');
  await r.step("guided-open", "with Docker not answering, the Overview offers the guided setup and opens it", async () => {
    await setUp.waitForClickable({ timeout: 60000, timeoutMsg: "no Set up this machine on the Overview" });
    await setUp.click();
    await switchToWindow(b, "machine-setup");
    await b.waitUntil(async () => /What do you want to run\?/.test(await b.$("body").getText()), {
      timeout: 60000,
      timeoutMsg: "the setup never asked what to run",
    });
  });
  await r.step("guided-path", "picking Container labs leads into its first step", async () => {
    await (await b.$('(//button[normalize-space()="Set up" or normalize-space()="Review"])[1]')).click();
    await b.waitUntil(async () => !/What do you want to run\?/.test(await b.$("body").getText()), { timeout: 30000, timeoutMsg: "the path never started" });
    return `now at: ${(await b.$("body").getText()).split("\n").find((l) => /Step \d+ of \d+/.test(l)) ?? "the path's first step"}`;
  });
  await closeWindow(b, "machine-setup");
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
