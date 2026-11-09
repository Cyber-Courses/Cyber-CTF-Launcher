// Failure scenario: the Cyber CTF backend is unreachable. Point CYBERBACKEND_URL at a dead host;
// the catalogue must degrade gracefully (no crash) and the UI must stay usable with its machine
// panel.
import { finish, goTo, mainText, openApp, sleep } from "../support.mjs";

/** What the Labs page shows once the catalogue load failed (any apostrophe). */
const DEGRADED = /can.?t (be )?reach|couldn|sign in|offline|no labs/i;

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp({ env: { CYBERBACKEND_URL: "http://127.0.0.1:1" } }));
  await goTo(b, "Labs");
  // The failed load can take a while to come back (a refused connection, retried): wait for it.
  let main = await mainText(b);
  for (let waited = 0; waited < 30000 && !DEGRADED.test(main); waited += 1000) {
    await sleep(1000);
    main = await mainText(b);
  }
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
  const degraded = DEGRADED.test(main);
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
