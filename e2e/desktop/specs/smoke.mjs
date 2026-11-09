// Real unmocked GUI smoke against the app's own EMBEDDED W3C WebDriver server
// (tauri-plugin-wdio-webdriver), which the app starts when launched with TAURI_WEBDRIVER_PORT.
// No external driver (tauri-driver/msedgedriver), so it works on macOS, Linux and Windows.
// The app is launched by the caller (run-embedded.sh / run-embedded.ps1); this only connects.
import { connect } from "../support.mjs";

const browser = await connect();
let rc = 0;
try {
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
  console.log("SMOKE_OK the app booted and its shell rendered (embedded WebDriver)");
} catch (e) {
  console.log(`SMOKE_FAIL ${e.message}`);
  rc = 1;
} finally {
  await browser.deleteSession().catch(() => {});
}
process.exit(rc);
