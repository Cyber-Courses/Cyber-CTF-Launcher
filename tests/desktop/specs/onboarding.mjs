// Setup: the first-run onboarding wizard renders inline in the main window and is drivable. Launch
// WITHOUT the onboarded flag so the wizard shows, confirm it renders, advance past Welcome, then
// Skip setup and assert the app completes onboarding and lands on the shell. Exercises the real
// wizard (the same machine-setup step components the standalone setup window reuses), without
// depending on the machine report, since Skip setup leaves the machine steps untouched.
import { connect, finish, forceVisible, launchApp, results, sleep } from "../lib/support.mjs";

const main = (b) =>
  b
    .$("main")
    .getText()
    .catch(() => "");
const clickText = async (b, label) => {
  const btn = await b.$(`button*=${label}`);
  if (!(await btn.isExisting())) return false;
  await btn.waitForClickable({ timeout: 10000 }).catch(() => {});
  await btn.click();
  return true;
};

export const needs = [];
const r = results();
let proc;
let b;
try {
  // Not onboarded from the start: the wdio build seeds the storage before the page's scripts run
  // (launchApp's `storage`; readOnboarded() is `=== "1"`, so "0" reads as not onboarded), and no
  // reload: the seed script runs once per launch, keyed by a `__wdio_` mark, and a spec that
  // cleared those keys and reloaded (this one, until 2026-11-12) had the seed run again with the
  // default, onboarded, and never saw the Welcome step.
  proc = await launchApp({ env: {}, storage: { "cyberctf.onboarded": "0" } });
  b = await connect();
  await forceVisible(b);
  // The Welcome step's "Get started" CTA (wizard-only; the shell's Overview also says "Welcome
  // to Cyber CTF", so anchor on the button, not the copy).
  const shown = await b
    .$("button*=Get started")
    .waitForClickable({ timeout: 20000 })
    .then(() => true)
    .catch(() => false);
  if (!shown) throw new Error(`onboarding Welcome never rendered (shell shown=${await b.$("aside nav").isExisting()}; main=${(await main(b)).slice(0, 140)})`);
  console.log("onboarding: wizard shown");
  await (await b.$("button*=Get started")).click();

  // Advancing past Welcome shows a "Back" control (or Get started is gone), proving navigation
  // without matching brittle per-step copy.
  await b.waitUntil(async () => (await b.$("button*=Back").isExisting()) || !(await b.$("button*=Get started").isExisting()), {
    timeout: 15000,
    timeoutMsg: "wizard did not advance past Welcome after Get started",
  });
  console.log("onboarding: advanced past Welcome");

  // 3. Skip the rest of setup; the app must complete onboarding and land on the shell. Skip can
  //    land on a final "done" step (Browse labs) before the shell, so chase either.
  if (!(await clickText(b, "Skip setup"))) throw new Error("no Skip setup to finish with");
  let done = false;
  for (let i = 0; i < 20 && !done; i++) {
    await forceVisible(b);
    if (await b.$("aside nav").isExisting()) {
      done = true;
      break;
    }
    await clickText(b, "Browse labs");
    await sleep(1000);
  }
  if (!done) throw new Error(`never reached the shell after Skip setup; main=${(await main(b)).slice(0, 160)}`);
  const onboarded = await b.execute(() => {
    try {
      return localStorage.getItem("cyberctf.onboarded");
    } catch {
      return null;
    }
  });
  r.pass("onboarding", `wizard rendered, advanced past Welcome, then Skip setup reached the shell (onboarded=${onboarded})`);
} catch (e) {
  r.fail("onboarding", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
