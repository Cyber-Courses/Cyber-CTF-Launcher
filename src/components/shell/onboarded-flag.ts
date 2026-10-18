// Whether this machine has been through the onboarding: a flag in localStorage. Without
// storage (a private window), the onboarding is taken as done, so it doesn't show every launch.

const ONBOARDED_KEY = "cyberctf.onboarded";

export function readOnboarded(): boolean {
  try {
    return localStorage.getItem(ONBOARDED_KEY) === "1";
  } catch {
    return true;
  }
}

export function markOnboarded() {
  try {
    localStorage.setItem(ONBOARDED_KEY, "1");
  } catch {
    /* ignore */
  }
}

/** The onboarding shows again on the next start (Settings' Replay onboarding). */
export function clearOnboarded() {
  try {
    localStorage.removeItem(ONBOARDED_KEY);
  } catch {
    /* ignore */
  }
}
