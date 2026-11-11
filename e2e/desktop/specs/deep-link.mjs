// Deep links: `cyberctf://labs/<slug>` shows that lab's page (never starts it). The link the app
// is opened with (on Linux and Windows the system passes it on the command line, which the
// deep-link plugin reads at start), and one opened while the app runs (the plugin's
// `deep-link://new-url` event, which this spec emits as the plugin does when the system hands a
// running app a link), for a catalogue lab (the first the Labs list shows; browsing needs no
// account) and an imported one when one is installed. A link to no lab, or not to a lab, leaves
// the app usable where it was. How the system routes the scheme to the app (the installers'
// registration, a second launch handed to the first) is the packaging smoke's.
import { finish, goTo, importedLabs, invokeCmd, mainButton, mainText, openApp, results, sleep } from "../support.mjs";

const r = results();
const ARGV_LINKS = process.platform !== "darwin";

/** The page's title, once it is `title`'s; false when it never got there. */
const showsLab = (b, title) =>
  b
    .waitUntil(
      async () =>
        (
          await b
            .$("main h1")
            .getText()
            .catch(() => "")
        ).includes(title),
      { timeout: 30000, interval: 1000 },
    )
    .then(() => true)
    .catch(() => false);
/** Hands the running app `url` the way the deep-link plugin does. */
const openLink = (b, url) => invokeCmd(b, "plugin:event|emit", { event: "deep-link://new-url", payload: [url] }, { timeoutMs: 15000 });

let proc;
let b;
try {
  // The labs to link to: the catalogue's first, and an imported one.
  ({ proc, b } = await openApp());
  await goTo(b, "Labs");
  const targets = [];
  const first = await b.$("main button[data-row-open]");
  if (await first.waitForExist({ timeout: 60000 }).catch(() => false)) {
    const title = (await first.getAttribute("aria-label")).replace(/^Open /, "");
    const slug = (await (await first.$("./following-sibling::span[1]")).getText()).split(/\s/)[0];
    targets.push({ kind: "catalogue", title, slug });
  }
  const imported = (await importedLabs(b))[0];
  if (imported) targets.push({ kind: "imported", title: imported.title, slug: imported.id });
  await finish(b, proc);
  b = null;
  proc = null;
  if (targets.length === 0) r.skipAll("no lab to link to: the catalogue can't be read here and no lab is imported");

  const [lab, other = lab] = targets;
  if (ARGV_LINKS) {
    ({ proc, b } = await openApp({ args: [`cyberctf://labs/${lab.slug}`] }));
    r.check("launch-link", await showsLab(b, lab.title), `opened with cyberctf://labs/${lab.slug}, the app shows ${lab.title}'s page (${lab.kind})`);
  } else {
    ({ proc, b } = await openApp());
    r.skip("launch-link", "macOS hands a link to the app as an event, not on its command line (the running-link checks cover the page's side)");
  }
  const offered = await mainButton(b, "Start lab", "Sign in to start", "Import a lab", "Retry").isExisting();
  r.check("not-started", offered, "the link only shows the lab: its page still offers to start it");

  await goTo(b, "Overview");
  await openLink(b, `cyberctf://labs/${other.slug}`);
  r.check("running-link", await showsLab(b, other.title), `a link opened while the app runs shows ${other.title}'s page (${other.kind})`);

  await goTo(b, "Overview");
  await openLink(b, "cyberctf://labs/no-such-lab-e2e");
  await sleep(3000);
  r.check("unknown-lab", (await b.$("aside nav").isExisting()) && !/Something went wrong/.test(await mainText(b)), "a link to no lab leaves the app usable");
  await goTo(b, "Overview");
  const before = await b.$("main h1").getText();
  await openLink(b, "cyberctf://elsewhere/thing");
  await sleep(3000);
  r.check("not-a-lab", (await b.$("main h1").getText()) === before, "a cyberctf:// link that isn't a lab's is ignored");
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 600)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
}
r.exit();
