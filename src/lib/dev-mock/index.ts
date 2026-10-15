/**
 * Browser preview of the UI (development only): `pnpm dev`, then open
 * http://localhost:3000/?mock. Outside Tauri there is no Rust core, so this answers the
 * commands the screens call with plausible sample data (sample.ts: a signed-in player, two
 * running labs, a server, an AWS account), changed as the preview is used (browser.ts) under the
 * switches in the URL (scenario.ts); Settings and the server setup open in new tabs. Never loaded
 * in a production build or inside the app. The labs and their objectives are in labs.ts and
 * objectives.ts.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { clearOnboarded, markOnboarded } from "@/components/shell/onboarded-flag";
import { browserAnswer } from "@/lib/dev-mock/browser";
import { joinEventBus } from "@/lib/dev-mock/events";
import { liveFrom } from "@/lib/dev-mock/live";
import { scenarioFrom } from "@/lib/dev-mock/scenario";
import type { Args } from "@/lib/dev-mock/types";

export { answer } from "@/lib/dev-mock/sample";
export { scenarioFrom, scenarioReport } from "@/lib/dev-mock/scenario";

/** What the preview adds to the window: whether it is installed, how a test brings the machine
 *  back under `check=down`, and how it lets a step it holds (`hold=`) go on. */
type MockWindow = { __CYBERCTF_MOCK__?: boolean; __CYBERCTF_MOCK_MACHINE_UP__?: () => void; __CYBERCTF_MOCK_RELEASE__?: (step: string) => void };

/** Installs the mock when running in a plain browser with ?mock in the URL. */
export function installDevMock() {
  if (process.env.NODE_ENV === "production" || typeof window === "undefined") return;
  const own = window as unknown as MockWindow;
  if ("__TAURI_INTERNALS__" in window && !own.__CYBERCTF_MOCK__) return;
  if (!new URLSearchParams(window.location.search).has("mock")) return;
  const sc = scenarioFrom(window.location.search);
  const live = liveFrom(sc);
  mockWindows("main");
  mockIPC((cmd, args) => browserAnswer(cmd, args as Args, { sc, live }));
  joinEventBus();
  own.__CYBERCTF_MOCK__ = true;
  own.__CYBERCTF_MOCK_MACHINE_UP__ = () => {
    live.checkDown = false;
  };
  own.__CYBERCTF_MOCK_RELEASE__ = live.holds.release;
  if (sc.onboarding) clearOnboarded();
  else markOnboarded();
}
