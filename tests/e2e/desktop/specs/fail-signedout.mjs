// Failure/edge scenario: a signed-out user. The app must stay fully usable (local labs) and prompt
// to sign in for website-launched labs, without errors.
//
// A debug build keeps its session in `$HOME/.cyberctf/dev-session.json`, so run.sh points
// HOME at an empty folder for this script: signed out whatever the developer's own session is.
// Run by hand, it relies on this machine having no session.
import { eventually, finish, goTo, openApp, results } from "../support/app.mjs";

export const needs = [];
const r = results();
let proc;
let b;
try {
  ({ proc, b } = await openApp());
  await goTo(b, "Overview");
  const body = () =>
    b
      .$("body")
      .getText()
      .catch(() => "");
  // The prompt shows once the app has read that no one is signed in (auth_status).
  const signInPrompt = await eventually(async () => /sign in/i.test(await body()), 30000);
  const usable = await b.$("main h1").isExisting();
  if (signInPrompt && usable) r.pass("fail-signedout", "signed-out app is usable and prompts to sign in");
  else {
    r.fail("fail-signedout", `asks to sign in: ${signInPrompt}, usable: ${usable}`);
  }
} catch (e) {
  r.fail("fail-signedout", e.message);
} finally {
  await finish(b, proc);
}
r.exit();
