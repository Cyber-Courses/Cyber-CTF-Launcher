// Functional GUI e2e: the app seeds a running lab on startup (CYBERCTF_WDIO_SEED_LAB, wdio build);
// this drives the REAL UI to verify the lab shows as running on the Overview.
import { existsSync } from "node:fs";
import { finish, goTo, mainText, openApp, waitForLab } from "../support.mjs";

const LAB = process.env.CYBERCTF_TEST_LOCAL_LAB;
if (!LAB || !existsSync(LAB)) {
  console.log(`no lab at CYBERCTF_TEST_LOCAL_LAB=${LAB}`);
  process.exit(2);
}

let rc = 0;
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
  console.log("FUNCTIONAL_LAUNCH_OK running lines:", JSON.stringify(running));
} catch (e) {
  console.log("FUNCTIONAL_LAUNCH_FAIL", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
