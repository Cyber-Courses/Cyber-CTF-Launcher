// Failure scenario: the Cyber CTF backend is unreachable. Point CYBERBACKEND_URL at a dead host;
// the catalogue must degrade gracefully (no crash) and the UI must stay usable with its machine
// panel.
import { finish, goTo, mainText, openApp, results, sleep } from "../lib/support.mjs";

/** What the Labs page shows once the catalogue load failed (any apostrophe). */
const DEGRADED = /can.?t (be )?reach|couldn|sign in|offline|no labs/i;

const r = results();
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
  if (rendered && degraded) r.pass("fail-offline", "the app stays usable and shows the catalogue degraded");
  else {
    r.fail("fail-offline", `shell rendered: ${rendered}, catalogue shown degraded: ${degraded}`);
  }
} catch (e) {
  r.fail("fail-offline", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
