// Failure/edge scenario: a signed-out user. The app must stay fully usable (local labs) and prompt
// to sign in for website-launched labs, without errors.
//
// A debug build keeps its session in `$HOME/.cyberctf/dev-session.json`, so run-all.sh points
// HOME at an empty folder for this script: signed out whatever the developer's own session is.
// Run by hand, it relies on this machine having no session.
import { finish, goTo, openApp, sleep } from "../support.mjs";

let rc = 0;
let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Overview");
  await sleep(4000);
  const body = await b
    .$("body")
    .getText()
    .catch(() => "");
  const signInPrompt = /sign in/i.test(body);
  const usable = await b.$("main h1").isExisting();
  if (signInPrompt && usable) console.log("FAIL_SIGNEDOUT_OK signed-out app is usable and prompts to sign in");
  else {
    console.log("FAIL_SIGNEDOUT_FAIL signIn=", signInPrompt, "usable=", usable);
    rc = 1;
  }
} catch (e) {
  console.log("FAIL_SIGNEDOUT_ERR", e.message);
  rc = 1;
} finally {
  await finish(b, proc);
}
process.exit(rc);
