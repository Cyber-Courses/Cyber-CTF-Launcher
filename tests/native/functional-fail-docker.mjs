// Failure scenario: the Docker engine is down. Point the app at a dead Docker socket; the UI must
// show the engine as not running and no lab running (the seed's launch cannot start).
import { finish, goTo, mainText, openApp, sleep } from "./lib.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
const env = { DOCKER_HOST: "unix:///nonexistent/docker.sock" };
if (LAB) env.CYBERCTF_WDIO_SEED_LAB = LAB;

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env }));
  await goTo(b, "Overview");
  await sleep(8000); // let the machine/docker checks run
  const main = await mainText(b);
  console.log("overview lines:", JSON.stringify(main.split("\n").filter((l) => /docker|engine|running|nothing|stopped/i.test(l)).slice(0, 10)));
  const dockerDown =
    /engine.*(stop|not running|off)|docker.*(stop|not running|off|unavailable)/i.test(main) || /start the engine|engine is off|engine stopped/i.test(main);
  const noLab = /Nothing running/i.test(main) || !/lab running on this machine/i.test(main);
  console.log("dockerDown:", dockerDown, "noLabRunning:", noLab);
  if (dockerDown && noLab) console.log("FAIL_DOCKER_OK the UI reflects Docker engine down + no lab running");
  else {
    console.log("FAIL_DOCKER_FAIL did not clearly reflect Docker down");
    rc = 1;
  }
} catch (e) {
  console.log("FAIL_DOCKER_ERR", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
