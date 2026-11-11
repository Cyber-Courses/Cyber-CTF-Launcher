// Real unmocked GUI smoke against the app's own EMBEDDED W3C WebDriver server
// (tauri-plugin-wdio-webdriver), which the app starts when launched with TAURI_WEBDRIVER_PORT.
// No external driver (tauri-driver/msedgedriver), so it works on macOS, Linux and Windows. The
// app is launched here as a player opens it: nothing seeded, nothing set.
import { connect, finish, launchApp, results } from "../lib/support.mjs";

export const needs = [];
const r = results();
let proc;
let browser;
try {
  proc = await launchApp();
  browser = await connect();
  await browser.waitUntil(
    async () => {
      const t = await browser
        .$("body")
        .then((b) => b.getText())
        .catch(() => "");
      return (t || "").length > 0;
    },
    { timeout: 30000, interval: 500, timeoutMsg: "the app shell did not render within 30s" },
  );
  const title = await browser.getTitle().catch(() => "");
  const bodyLen = (
    await browser
      .$("body")
      .then((b) => b.getText())
      .catch(() => "")
  ).length;
  console.log(`SMOKE title=${JSON.stringify(title)} bodyLen=${bodyLen}`);
  r.pass("smoke", "the app booted and its shell rendered (embedded WebDriver)");
} catch (e) {
  r.fail("smoke", `${e.message}`);
  if (proc) console.log("app out tail:", proc.output().slice(-600));
} finally {
  await finish(browser, proc);
}
r.exit();
