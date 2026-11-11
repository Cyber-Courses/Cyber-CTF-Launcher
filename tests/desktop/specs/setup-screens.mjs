// Setup: the Servers and Cloud screens render and offer their setup entry points: their titles and
// the "Add server" / "Set up cloud provider" buttons, on a signed-out machine with no hosts
// configured, the shell staying usable. It doesn't open the wizards: the `server-lab` spec drives
// the server setup window (`switchToWindow(b, "server-setup")`), and the web specs
// (setup-wizards.spec.ts) the server and cloud setups against the dev mock.
import { finish, goTo, mainText, openApp, results } from "../lib/support.mjs";

const r = results();
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: {} }));

  // The app may still be "Checking this machine…" when the shell first renders, so retry the nav
  // and wait for the target screen's own copy rather than assume the first click lands.
  const navWait = async (tab, needle) => {
    for (let i = 0; i < 5; i++) {
      await goTo(b, tab);
      try {
        await b.waitUntil(async () => needle.test(await mainText(b)), { timeout: 6000 });
        return await mainText(b);
      } catch {
        /* retry the nav */
      }
    }
    throw new Error(`${tab} screen never loaded; main=${(await mainText(b)).slice(0, 200)}`);
  };

  const servers = await navWait("Servers", /multi-machine labs|Add server|No server connected/i);
  if ((await b.$$("button*=Add server")).length === 0) throw new Error("no Add server button on the Servers screen");
  console.log("servers screen ok:", JSON.stringify(servers.replace(/\s+/g, " ").slice(0, 120)));

  const cloud = await navWait("Cloud", /Throwaway instances|Set up cloud provider/i);
  if ((await b.$$("button*=Set up cloud provider")).length === 0) throw new Error("no Set up cloud provider button on the Cloud screen");
  console.log("cloud screen ok:", JSON.stringify(cloud.replace(/\s+/g, " ").slice(0, 120)));

  // Still usable afterwards.
  await goTo(b, "Overview");
  if (!(await b.$("aside nav").isExisting())) throw new Error("shell unusable after visiting Servers/Cloud");
  r.pass("setup-screens", "Servers and Cloud screens render with their setup entry points");
} catch (e) {
  r.fail("setup-screens", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
