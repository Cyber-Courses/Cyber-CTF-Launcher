// The sign-in in the browser preview, shared by its tabs as the core's one session is: a sign-in
// (or sign-out) in Settings reaches the main window's next `auth_status`. Kept in the browser's
// storage, like the saved servers (hosts.ts); until one happens, `auth=out` decides.
import type { Scenario } from "@/lib/dev-mock/scenario";

const SESSION_KEY = "cyberctf.devMock.signedIn";

/** Whether the player is signed in: as last signed in or out in the preview, else the scenario. */
export function isSignedIn(sc: Scenario): boolean {
  try {
    const saved = localStorage.getItem(SESSION_KEY);
    if (saved !== null) return saved === "1";
  } catch {
    /* storage unavailable */
  }
  return !sc.signedOut;
}

/** Keeps a sign-in (`true`) or a sign-out for every tab of the preview. */
export function setSignedIn(signedIn: boolean) {
  try {
    localStorage.setItem(SESSION_KEY, signedIn ? "1" : "0");
  } catch {
    /* storage unavailable: the sign-in is not kept */
  }
}
