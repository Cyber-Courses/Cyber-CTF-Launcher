// Failure scenario: the Docker engine is unreachable. The app gets a dead Docker socket and a Docker
// config without a current context (see deadDockerEnv), plus the seed lab it would normally start
// on launch: the header must say Docker is not running, the seed's start must fail, and the
// Overview must show nothing running.
import { LAB_RUNNING, deadDockerEnv, finish, goTo, mainText, openApp, waitAwake } from "./lib.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
const env = deadDockerEnv();
if (LAB) env.CYBERCTF_WDIO_SEED_LAB = LAB;

const DOCKER_DOWN = /Docker not running/i;
const NOTHING_RUNNING = /Nothing running/i;

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env }));
  await goTo(b, "Overview");
  // The machine check reports the engine within a few seconds; the seed's failed start may
  // flash a toast but must leave nothing running.
  await waitAwake(b, async () => DOCKER_DOWN.test(await mainText(b)), {
    timeout: 40000,
    interval: 2000,
    timeoutMsg: "the header never said Docker is not running",
  });
  await waitAwake(b, async () => NOTHING_RUNNING.test(await mainText(b)), {
    timeout: 20000,
    interval: 2000,
    timeoutMsg: "the Overview never settled on nothing running",
  });
  const main = await mainText(b);
  const dockerDown = DOCKER_DOWN.test(main);
  const noLab = NOTHING_RUNNING.test(main) && !LAB_RUNNING.test(main);
  console.log(
    "overview lines:",
    JSON.stringify(
      main
        .split("\n")
        .filter((l) => /docker|running|nothing/i.test(l))
        .slice(0, 6)
        .map((l) => l.slice(0, 200)),
    ),
  );
  console.log("dockerDown:", dockerDown, "noLabRunning:", noLab);
  if (dockerDown && noLab) console.log("FAIL_DOCKER_OK the UI reflects Docker engine down + no lab running");
  else {
    console.log("FAIL_DOCKER_FAIL did not clearly reflect Docker down");
    rc = 1;
  }
} catch (e) {
  console.log("FAIL_DOCKER_FAIL", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b).catch(() => "")).slice(0, 600)));
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
