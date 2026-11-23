import { readStored, writeStored } from "@/lib/local-store";

// Whether this machine has been through the onboarding: a flag in localStorage. Without
// storage (a private window), the onboarding is taken as done, so it doesn't show every launch.

const ONBOARDED_KEY = "cyberctf.onboarded";

/** Storage that can't be read at all reads as done (see above). */
export const readOnboarded = (): boolean => readStored(ONBOARDED_KEY, "1") === "1";

export const markOnboarded = () => {
  writeStored(ONBOARDED_KEY, "1");
};

/** The onboarding shows again on the next start (Settings' Replay onboarding). */
export const clearOnboarded = () => {
  writeStored(ONBOARDED_KEY, null);
};
