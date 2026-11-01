// Setup: the Servers and Cloud screens render and offer their setup entry points. The wizards they
// open run in a separate OS window the embedded WebDriver cannot reach, so this asserts the list
// screens themselves — their titles and the "Add server" / "Set up cloud provider" buttons — and
// that the shell stays usable (a signed-out machine with no hosts configured).
import { finish, goTo, mainText, openApp } from "./lib.mjs";

let rc = 0;
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
  console.log("SETUP_SCREENS_OK Servers and Cloud screens render with their setup entry points");
} catch (e) {
  console.log("SETUP_SCREENS_FAIL", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
