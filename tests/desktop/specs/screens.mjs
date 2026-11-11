// A VM's screen (its graphical console) in its own window, from a running VM lab's page: the
// Screens panel lists the lab's VMs; one whose run gives a VNC console opens a Screen window
// (`screen-<lab>:<machine>`) that shows its framebuffer (noVNC drawing into a canvas through the
// window's loopback bridge); one with Remote Desktop opens the window that hands it to the
// system's client (not launched here); one with no screen says so on its row, and why. With
// CYBERCTF_TEST_BOX_DESKTOP=1 the attack box is Kali's desktop (Settings > Labs, Attack VM
// desktop), and its own Screen shows it. Then the lab is stopped and removed.
//
//   CYBERCTF_TEST_VM_PROVIDER       the hypervisor to run the lab on (libvirt and QEMU give VNC)
//   CYBERCTF_TEST_CATALOGUE_VM_LAB  a catalogue VM lab (else the container lab as one VM per machine)
//   CYBERCTF_TEST_BOX_DESKTOP    1: also the attack box's desktop (about 1 GB more, 4 GB of memory)
// Needs the app signed in. The console's plumbing per hypervisor (the runner's answer, the bridge)
// is unit-tested in the core; a cloud's or a server's screen is the integration tests' to add.
import {
  CATALOGUE_LAB,
  closeWindow,
  finish,
  forceVisible,
  mainButton,
  mainText,
  openApp,
  openLabFromList,
  results,
  signedIn,
  startFromDialog,
  stopAndRemoveOnPage,
  waitAwake,
  waitLabPageRunning,
} from "../lib/support.mjs";

const r = results();
const PROVIDER = process.env.CYBERCTF_TEST_VM_PROVIDER || (process.platform === "darwin" ? "utm" : "");
const VM_LAB = process.env.CYBERCTF_TEST_CATALOGUE_VM_LAB || "";
const DESKTOP = process.env.CYBERCTF_TEST_BOX_DESKTOP === "1";
if (!PROVIDER) r.skipAll("no hypervisor named: set CYBERCTF_TEST_VM_PROVIDER");
const LAB = VM_LAB || CATALOGUE_LAB;
const START = VM_LAB ? { place: "vm", form: "one" } : { place: "vm", form: "per-machine" };

/** The Screens panel's rows: each machine's name, its note, and whether it has a Screen button. */
async function screenRows(b) {
  const rows = await b.$$('//main//h3[normalize-space()="Screens"]/following::ul[1]/li');
  return Promise.all(
    rows.map(async (li) => {
      const [name, note = ""] = (await li.getText()).split("\n");
      return { name, note, opens: await li.$(".//button").isExisting() };
    }),
  );
}

/** Opens a Screen window with `click`, waits for it to settle; what it shows. */
async function openScreen(b, click) {
  const before = await b.getWindowHandles();
  await click();
  let label;
  await b.waitUntil(async () => (label = (await b.getWindowHandles()).find((h) => !before.includes(h) && h.startsWith("screen-"))) !== undefined, {
    timeout: 30000,
    timeoutMsg: "no Screen window opened",
  });
  await b.switchToWindow(label);
  await forceVisible(b);
  let state = "";
  await b.waitUntil(
    async () =>
      !/connecting/.test(
        (state = await b
          .$("body")
          .getText()
          .catch(() => "")),
      ),
    { timeout: 120000, interval: 2000, timeoutMsg: "the screen stayed connecting" },
  );
  const frame = await b.execute(() => {
    const c = document.querySelector('[data-testid="screen-host"] canvas');
    if (!c) return null;
    let lit = 0;
    try {
      const d = c.getContext("2d").getImageData(0, 0, Math.min(c.width, 400), Math.min(c.height, 300)).data;
      for (let i = 0; i < d.length; i += 4) if (d[i] || d[i + 1] || d[i + 2]) lit++;
    } catch {
      /* a canvas that can't be read still drew */
    }
    return { width: c.width, height: c.height, lit };
  });
  await closeWindow(b, label);
  await forceVisible(b);
  return { label, state: state.replace(/\s+/g, " ").slice(0, 300), frame };
}
/** A framebuffer drawn: the window showing the screen, its canvas sized by the machine. */
const showing = (seen) => /· screen/.test(seen.state) && seen.frame && seen.frame.width > 0 && seen.frame.height > 0;
const described = (seen) => (seen.frame ? `${seen.frame.width}x${seen.frame.height}, ${seen.frame.lit} lit pixels in its corner` : seen.state);

let proc;
let b;
try {
  ({ proc, b } = await openApp({ storage: DESKTOP ? { "cyberctf.attackbox.desktop": "true" } : {} }));
  if (!(await signedIn(b))) r.skipAll("the app isn't signed in: a catalogue lab starts only signed in (give CYBERCTF_TEST_ACCOUNT_SESSION to the runner)");
  if (!(await openLabFromList(b, LAB))) r.skipAll(`the catalogue doesn't list "${LAB}"`);
  if (await (await mainButton(b, "Stop & remove", "Stop & clean up")).isExisting()) await stopAndRemoveOnPage(b);
  await startFromDialog(b, { ...START, provider: PROVIDER });
  await waitLabPageRunning(b, 3600000);

  let rows = [];
  await r.step("panel", "the running lab's page lists its VMs' screens", async () => {
    await waitAwake(b, async () => (rows = await screenRows(b)).length > 0, { timeout: 120000, interval: 5000, timeoutMsg: "no Screens panel on the page" });
    return rows.map((m) => `${m.name}: ${m.note}`).join("; ");
  });
  const vnc = rows.find((m) => m.opens && /in a window/.test(m.note));
  if (vnc) {
    await r.step("screen", `${vnc.name}'s Screen window shows its framebuffer`, async () => {
      const seen = await openScreen(b, async () => (await b.$(`main button[aria-label=${JSON.stringify(`Open ${vnc.name}'s screen`)}]`)).click());
      if (!showing(seen)) throw new Error(`${seen.label}: ${seen.state}`);
      return `${seen.label}: ${described(seen)}`;
    });
  } else r.skip("screen", `no machine with a VNC screen on ${PROVIDER} (${rows.map((m) => `${m.name}: ${m.note}`).join("; ") || "none listed"})`);
  const rdp = rows.find((m) => m.opens && /Remote Desktop/.test(m.note));
  if (rdp) {
    await r.step("screen-rdp", `${rdp.name}'s Screen window hands it to Remote Desktop`, async () => {
      const seen = await openScreen(b, async () => (await b.$(`main button[aria-label=${JSON.stringify(`Open ${rdp.name}'s screen`)}]`)).click());
      if (!/opens in Remote Desktop|No Remote Desktop app is installed/.test(seen.state)) throw new Error(seen.state);
      return seen.state;
    });
  } else r.skip("screen-rdp", "no machine with a Remote Desktop here");
  const none = rows.find((m) => !m.opens);
  if (none) r.check("screen-none", /^No screen: \S|opens once it runs/.test(none.note), `a machine without a screen says why: ${none.name}: ${none.note}`);
  else r.skip("screen-none", "every machine here has a screen");

  if (DESKTOP) {
    await r.step("box-desktop", "the attack box with Kali's desktop: its Screen shows it", async () => {
      const start = await b.$('//main//aside//button[contains(normalize-space(), "Start attack box")]');
      if (await start.isExisting()) await start.click();
      const open = await b.$('//main//aside//button[contains(normalize-space(), "Open screen")]');
      await waitAwake(b, () => open.isExisting(), { timeout: 3600000, interval: 10000, timeoutMsg: "the box never offered its screen" });
      const seen = await openScreen(b, () => open.click());
      if (!showing(seen)) throw new Error(`${seen.label}: ${seen.state}`);
      return `${seen.label}: ${described(seen)}`;
    });
  } else r.skip("box-desktop", "the attack box's desktop downloads about 1 GB more and takes 4 GB: set CYBERCTF_TEST_BOX_DESKTOP=1");

  await r.step("stop", "Stop & remove ends the lab, its screens with it", () => stopAndRemoveOnPage(b, 1800000));
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  // Headless again, the setting's default, for the specs after this one.
  if (DESKTOP && b) await b.execute(() => localStorage.setItem("cyberctf.attackbox.desktop", "false")).catch(() => {});
  await finish(b, proc);
}
r.exit();
