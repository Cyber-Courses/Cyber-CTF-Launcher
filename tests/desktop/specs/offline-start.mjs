// Offline start: a catalogue lab started once online, then started again while Cyber CTF can't be
// reached. The app reaches the backend through a relay this spec runs (CYBERBACKEND_URL pointed at
// it), so the network can drop mid-session as it does for a player: the lab is started and
// stopped online (which keeps the source of its start, src-tauri/src/labs/offline.rs), the relay
// is cut, and Start then asks "Start <lab> offline?"; Start offline runs the lab from the copy
// installed here, saying so in its log, and it stops again. A backend that answers is never
// bypassed (a unit test's); a cloud lab never starts offline.
//
//   CYBERCTF_TEST_CATALOGUE_LAB  the catalogue container lab (default Supplier portal API)
//   CYBERBACKEND_URL             the backend the relay forwards to (default https://cyberbackend.com)
// Needs the app signed in and a Docker engine.
import http from "node:http";
import https from "node:https";
import {
  CATALOGUE_LAB,
  dockerUp,
  finish,
  LAB_START_MS,
  mainButton,
  mainText,
  openApp,
  openLabFromList,
  results,
  signedIn,
  startFromDialog,
  STOP_MS,
  stopAndRemoveOnPage,
  stopLeftover,
  waitAwake,
  waitLabPageRunning,
} from "../lib/support.mjs";

export const needs = ["catalogue-lab"];
const r = results();
if (!dockerUp()) r.skipAll("no Docker engine answers here");
const UPSTREAM = new URL(process.env.CYBERBACKEND_URL || "https://cyberbackend.com");

/** A relay to the backend on a free local port; `cut()` closes it and every connection through
 *  it, so the next call is refused, as with no network. */
async function relay() {
  const client = UPSTREAM.protocol === "https:" ? https : http;
  const sockets = new Set();
  const server = http.createServer((req, res) => {
    const out = client.request(
      { hostname: UPSTREAM.hostname, port: UPSTREAM.port || undefined, path: req.url, method: req.method, headers: { ...req.headers, host: UPSTREAM.host } },
      (answer) => {
        res.writeHead(answer.statusCode ?? 502, answer.headers);
        answer.pipe(res);
      },
    );
    out.on("error", () => {
      res.writeHead(502);
      res.end();
    });
    req.pipe(out);
  });
  server.on("connection", (s) => {
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    cut: () =>
      new Promise((resolve) => {
        server.close(() => resolve());
        for (const s of sockets) s.destroy();
      }),
  };
}

let proc;
let b;
let backend;
try {
  backend = await relay();
  ({ proc, b } = await openApp({ env: { CYBERBACKEND_URL: backend.url } }));
  if (!(await signedIn(b)))
    r.skipAll(
      "the app isn't signed in: an offline start is kept for the account that started the lab online (give CYBERCTF_TEST_ACCOUNT_SESSION to the runner)",
    );
  if (!(await openLabFromList(b, CATALOGUE_LAB))) r.skipAll(`the catalogue doesn't list "${CATALOGUE_LAB}"`);
  await stopLeftover(b, r);

  const online = await r.step("online", "started and stopped online first (the start's source kept)", async () => {
    await startFromDialog(b, { place: "container" });
    await waitLabPageRunning(b, LAB_START_MS);
    await stopAndRemoveOnPage(b);
  });
  if (!online) r.skipAll("the lab couldn't be started online, so there is nothing to start offline");

  await backend.cut();
  await r.step("prompt", "with the backend unreachable, Start asks to start offline", async () => {
    await startFromDialog(b, { place: "container" });
    const ask = await b.$('//*[@role="dialog" or @role="alertdialog"][.//button[normalize-space()="Start offline"]]');
    await ask.waitForExist({ timeout: 180000, timeoutMsg: `no offline question: ${(await mainText(b)).slice(0, 400)}` });
    return (await ask.getText()).split("\n")[0];
  });
  await r.step("offline-run", "Start offline runs the installed copy and its log says so", async () => {
    await (await b.$('//button[normalize-space()="Start offline"]')).click();
    await waitAwake(
      b,
      async () =>
        /Starting offline/.test(
          await b
            .$("main")
            .getText()
            .catch(() => ""),
        ) || (await mainButton(b, "Stop & remove").isExisting()),
      {
        timeout: STOP_MS,
        timeoutMsg: "the offline start never began",
      },
    );
    await waitLabPageRunning(b, LAB_START_MS);
  });
  await r.step("offline-stop", "the offline lab stops and is removed", () => stopAndRemoveOnPage(b));
} catch (e) {
  r.fail("error", e.message);
  if (b) console.log("main text:", JSON.stringify((await mainText(b)).slice(0, 800)));
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(b, proc);
  await backend?.cut().catch(() => {});
}
r.exit();
