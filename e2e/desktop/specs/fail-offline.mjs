// Failure scenario: the Cyber CTF backend is unreachable. Point CYBERBACKEND_URL at a dead host;
// the catalogue must degrade gracefully (no crash) and the UI must stay usable with its machine
// panel.
import { finish, goTo, mainText, openApp, sleep } from "../support.mjs";

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: { CYBERBACKEND_URL: "http://127.0.0.1:1" } }));
  await goTo(b, "Labs");
  await sleep(10000);
  const main = await mainText(b);
  console.log(
    "labs lines:",
    JSON.stringify(
      main
        .split("\n")
        .filter((l) => /reach|offline|sign in|couldn|labs|catalog/i.test(l))
        .slice(0, 8),
    ),
  );
  const rendered = await b.$("aside nav").isExisting(); // app still usable, not crashed
  const degraded = /can.?t be reached|couldn|sign in|offline|no labs|loading labs/i.test(main);
  if (rendered && degraded) console.log("FAIL_OFFLINE_OK the app stays usable and shows the catalogue degraded");
  else {
    console.log("FAIL_OFFLINE_FAIL rendered=", rendered, "degraded=", degraded);
    rc = 1;
  }
} catch (e) {
  console.log("FAIL_OFFLINE_ERR", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
