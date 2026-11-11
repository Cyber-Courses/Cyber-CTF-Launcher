// Functional GUI e2e: the app seeds a running lab on startup (CYBERCTF_WDIO_SEED_LAB, wdio build);
// this drives the REAL UI to verify the lab shows as running on the Overview.
import { finish, goTo, localLab, mainText, openApp, results, waitForLab } from "../lib/support.mjs";

const r = results();
const LAB = localLab();
if (!LAB) r.skipAll("no lab folder: set CYBERCTF_TEST_LOCAL_LAB to an Isoloom lab");

let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: { CYBERCTF_WDIO_SEED_LAB: LAB } }));
  await b.$("main h1").waitForExist({ timeout: 25000 });
  console.log("shell rendered; h1 =", JSON.stringify(await b.$("main h1").getText()));
  await goTo(b, "Overview");
  await waitForLab(b, 220000, "the seeded lab never showed as running on Overview");
  const running = (await mainText(b))
    .split("\n")
    .filter((l) => /running/i.test(l))
    .slice(0, 4);
  r.pass("launch", `the seeded lab runs on the Overview: ${running.join(" | ")}`);
} catch (e) {
  r.fail("launch", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
