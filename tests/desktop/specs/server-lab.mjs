// A Proxmox or ESXi server added through Servers > Add server, its setup window (`server-setup`)
// filled from a settings file the way a player types them: the hypervisor, the tools this machine
// needs for it (installed from the window when it can), the address and sign-in, the placement,
// then Save and test, whose checks must pass. Proxmox's real VM test runs there at once; ESXi's
// from the server's row. The server is listed on the Servers screen and its Test answers; signed
// in, a catalogue lab starts on it through the lab page's dialog, runs, and is stopped and removed.
// Last, the server is removed (its confirmation answered) and leaves the list.
//
//   CYBERCTF_TEST_SERVER_ENV     a KEY=value file (read literally, as the test rig writes them):
//     Proxmox  CYBERCTF_TEST_PVE_HOST, CYBERCTF_TEST_PVE_TOKEN_ID, CYBERCTF_TEST_PVE_TOKEN_SECRET,
//              optional CYBERCTF_TEST_PVE_NODE, _STORAGE, _BRIDGE (when the server's own lists
//              can't be read)
//     ESXi     CYBERCTF_TEST_ESXI_HOST, CYBERCTF_TEST_ESXI_PASSWORD, optional _USER (root),
//              _DATASTORE, _NETWORK
//   CYBERCTF_TEST_SERVER_LAB     the catalogue lab to start there (default CYBERCTF_TEST_CATALOGUE_LAB)
// The same knobs' variables in the environment work too. A lab's whole life on a server (status,
// checks over SSH, provisioning) is the integration tests' server tier (server-lab-proxmox, server-lab-esxi).
import { existsSync, readFileSync } from "node:fs";
import {
  CATALOGUE_LAB,
  closeWindow,
  confirmSystemDialogs,
  finish,
  goTo,
  mainText,
  openApp,
  openLabFromList,
  results,
  signedIn,
  sleep,
  startFromDialog,
  stopAndRemoveOnPage,
  switchToWindow,
  waitAwake,
  waitLabPageRunning,
} from "../lib/support.mjs";

const r = results();

/** The settings: the file's KEY=value lines (values as written), over the environment's. */
function settings() {
  const file = process.env.CYBERCTF_TEST_SERVER_ENV;
  const env = { ...process.env };
  if (file && existsSync(file)) {
    for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
      if (!line || line.startsWith("#") || !line.includes("=")) continue;
      env[line.slice(0, line.indexOf("="))] = line.slice(line.indexOf("=") + 1);
    }
  }
  return env;
}
const env = settings();
const KIND = env.CYBERCTF_TEST_PVE_HOST && env.CYBERCTF_TEST_PVE_TOKEN_ID ? "proxmox" : env.CYBERCTF_TEST_ESXI_HOST ? "esxi" : null;
if (!KIND)
  r.skipAll("no server: set CYBERCTF_TEST_SERVER_ENV to a file with CYBERCTF_TEST_PVE_HOST/_TOKEN_ID/_TOKEN_SECRET or CYBERCTF_TEST_ESXI_HOST/_PASSWORD");
const NAME = `e2e ${KIND} ${process.pid}`;
const LAB = env.CYBERCTF_TEST_SERVER_LAB || CATALOGUE_LAB;

const field = (b, label) => b.$(`//label[span[normalize-space()=${JSON.stringify(label)}]]//input`);
const button = (b, label) => b.$(`//button[normalize-space()=${JSON.stringify(label)}]`);
const body = (b) =>
  b
    .$("body")
    .getText()
    .catch(() => "");
/** Types `value` into the field labelled `label`, when the step has it. */
async function fill(b, label, value) {
  const input = await field(b, label);
  if (value && (await input.isExisting())) await input.setValue(value);
}
/** Continue, once enabled. */
async function next(b) {
  const go = await button(b, "Continue");
  await go.waitForClickable({ timeout: 60000, timeoutMsg: `Continue stays disabled:\n${(await body(b)).slice(0, 800)}` });
  await go.click();
  await sleep(1000);
}

/** The tools step: what's missing installed from the window (Terraform, Vagrant, the ESXi
 *  plugin), then Continue; what it can't install (the OVF Tool) fails the step, named. */
async function tools(b) {
  for (let i = 0; i < 4; i++) {
    const go = await button(b, "Continue");
    await b.waitUntil(async () => !/Checking this machine/.test(await body(b)), { timeout: 120000 });
    if (await go.isEnabled()) return "every tool there";
    const install = await b.$('//button[starts-with(normalize-space(), "Install") and not(@disabled)]');
    if (!(await install.isExisting())) break;
    const what = await install.getText();
    await install.click();
    await b.waitUntil(async () => !/Installing/.test(await body(b)), { timeout: 1200000, timeoutMsg: `${what} never finished` });
    await (await button(b, "Re-check")).click().catch(() => {});
    await sleep(3000);
  }
  if (await (await button(b, "Continue")).isEnabled()) return "tools installed from the window";
  throw new Error(`tools missing that the window can't install:\n${(await body(b)).slice(0, 800)}`);
}

let proc;
let b;
let added = false;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Servers");
  await (await b.$('//main//button[contains(normalize-space(), "Add server")]')).click();
  await switchToWindow(b, "server-setup");

  await (await b.$(`//*[@role="radio"][contains(normalize-space(), ${JSON.stringify(KIND === "proxmox" ? "Proxmox" : "VMware ESXi")})]`)).click();
  await next(b);
  const ready = await r.step("tools", "the tools this machine needs for the server", () => tools(b));
  if (!ready) {
    await finish(b, proc);
    r.exit();
  }
  await next(b);

  if (KIND === "proxmox") {
    await fill(b, "Name", NAME);
    await fill(b, "Host", env.CYBERCTF_TEST_PVE_HOST);
    await next(b);
    await (await b.$('//*[@role="radiogroup"][@aria-label="Sign-in method"]//*[@role="radio"][normalize-space()="API token"]')).click();
    await fill(b, "Token id", env.CYBERCTF_TEST_PVE_TOKEN_ID);
    await fill(b, "Token secret", env.CYBERCTF_TEST_PVE_TOKEN_SECRET);
    await next(b);
    await b.waitUntil(async () => !/Reading the server's nodes/.test(await body(b)), { timeout: 120000 });
    // Typed only where the server's own lists couldn't be read (the fields then are text).
    await fill(b, "Node", env.CYBERCTF_TEST_PVE_NODE);
    await fill(b, "VM disks", env.CYBERCTF_TEST_PVE_STORAGE);
    await fill(b, "Bridge", env.CYBERCTF_TEST_PVE_BRIDGE);
  } else {
    await fill(b, "Name", NAME);
    await fill(b, "Host", env.CYBERCTF_TEST_ESXI_HOST);
    await fill(b, "Username", env.CYBERCTF_TEST_ESXI_USER || "root");
    await fill(b, "Password", env.CYBERCTF_TEST_ESXI_PASSWORD);
    await next(b);
    await fill(b, "Datastore", env.CYBERCTF_TEST_ESXI_DATASTORE);
    await fill(b, "Port group", env.CYBERCTF_TEST_ESXI_NETWORK);
  }
  await (await button(b, "Save and test")).click();
  added = true;
  await r.step("test", "Save and test: the connection's checks pass", async () => {
    const mark = await b.$('//*[@role="img"][@aria-label="Passed" or @aria-label="Passed, with a warning" or @aria-label="Failed"]');
    await mark.waitForExist({ timeout: 300000, timeoutMsg: "the connection test never answered" });
    const outcome = await mark.getAttribute("aria-label");
    const text = (await body(b)).replace(/\s+/g, " ");
    if (outcome === "Failed") throw new Error(text.slice(0, 800));
    return `${outcome}: ${text.match(/What was checked.*$/)?.[0]?.slice(0, 300) ?? ""}`;
  });
  if (KIND === "proxmox") {
    await r.step("vm-test", "the real VM test on the server (one small VM on its own network, then removed)", () =>
      b.waitUntil(
        async () => {
          const t = await body(b);
          if (/VM test (failed|incomplete)/.test(t)) throw new Error(t.slice(t.search(/VM test/), t.search(/VM test/) + 800));
          return /VM test passed/.test(t);
        },
        { timeout: 2400000, interval: 10000, timeoutMsg: "the VM test never ended" },
      ),
    );
  }
  await (await button(b, "Done")).click().catch(() => {});
  await closeWindow(b, "server-setup");

  await goTo(b, "Servers");
  const row = () => b.$(`//main//*[normalize-space()=${JSON.stringify(NAME)}]/ancestor::div[.//button[normalize-space()="Test"]][1]`);
  await r.step("listed", "the server is listed and its Test says Online", async () => {
    await (await row()).waitForExist({ timeout: 30000, timeoutMsg: `${NAME} isn't on the Servers screen` });
    await (await (await row()).$('.//button[normalize-space()="Test"]')).click();
    await waitAwake(b, async () => /Online|Unreachable|Needs password/.test(await (await row()).getText()), {
      timeout: 120000,
      timeoutMsg: "its Test never answered",
    });
    const text = await (await row()).getText();
    if (!/Online/.test(text)) throw new Error(text.replace(/\s+/g, " "));
  });
  if (KIND === "esxi") {
    await r.step("vm-test", "the real VM test on the server, from its row", async () => {
      await (await (await row()).$('.//button[@aria-label="More actions"]')).click();
      await (await b.$('//button[normalize-space()="VM test"]')).click();
      await waitAwake(
        b,
        async () => {
          const t = await mainText(b);
          if (/VM test (failed|incomplete)/.test(t)) throw new Error(t.slice(t.search(/VM test/), t.search(/VM test/) + 800));
          return /VM test passed/.test(t);
        },
        { timeout: 2400000, interval: 10000, timeoutMsg: "the VM test never ended" },
      );
    });
  }

  if (!(await signedIn(b))) {
    for (const id of ["lab-start", "lab-running", "lab-stop"])
      r.skip(id, "the app isn't signed in: a catalogue lab starts only signed in (CYBERCTF_TEST_ACCOUNT_SESSION)");
  } else if (!(await openLabFromList(b, LAB))) {
    for (const id of ["lab-start", "lab-running", "lab-stop"]) r.skip(id, `the catalogue doesn't list "${LAB}" (set CYBERCTF_TEST_SERVER_LAB)`);
  } else {
    const started = await r.step(
      "lab-start",
      `${LAB} started on ${NAME} through the dialog`,
      async () => `picked: ${await startFromDialog(b, { place: "host", host: NAME })}`,
    );
    const running = started && (await r.step("lab-running", `${LAB} runs on the server, as its page says`, () => waitLabPageRunning(b, 3600000)));
    if (started) await r.step("lab-stop", "Stop & remove takes it off the server", () => stopAndRemoveOnPage(b, 1800000));
    else r.skip("lab-stop", "the lab never started");
    if (!running) console.log("(the lab didn't reach running; see lab-running)");
  }

  await goTo(b, "Servers");
  await r.step("remove", "Remove server (confirmed) takes it off the list", async () => {
    await confirmSystemDialogs(b);
    await (await (await row()).$('.//button[@aria-label="More actions"]')).click();
    await (await b.$('//button[normalize-space()="Remove"]')).click();
    await b.waitUntil(async () => !(await (await row()).isExisting()), { timeout: 30000, timeoutMsg: `${NAME} is still listed` });
    added = false;
  });
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("window text:", JSON.stringify((await body(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  if (added && b) console.log(`(the server "${NAME}" may be left in the app's saved servers)`);
  await finish(b, proc);
}
r.exit();
