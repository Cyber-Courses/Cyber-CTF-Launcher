"use client";

import { LOCAL_PROVIDERS } from "@/lib/tauri";
import type { LabRunOptions, LabRunRefusalCause, Provider } from "@/lib/tauri";
import type { T } from "@/lib/i18n";

// Why the lab is refused on a place of "Where should it run?": where Isoloom refuses it
// (`labRunOptions`), or where this computer's policy turns it off.

/** Why the lab is refused on `provider` (none: as containers on this machine's Docker), if it is:
 *  Isoloom's reason, or the policy setting that turns it off. */
export function refusalOn(t: T, options: LabRunOptions | null, provider: Provider | null): string | undefined {
  const refusal = options?.refused.find((r) => r.provider === provider);
  return refusal && said(t, refusal.cause);
}

/** Why the lab runs on none of this machine's hypervisors, as the first refusal of one says. */
export function refusalOnHypervisors(t: T, options: LabRunOptions | null): string | undefined {
  const refusal = options?.refused.find((r) => r.provider !== null && LOCAL_PROVIDERS.includes(r.provider));
  return refusal && said(t, refusal.cause);
}

/** A refusal's `cause` in the player's words, naming who refuses: Isoloom, or the organization's
 *  policy. */
function said(t: T, cause: LabRunRefusalCause): string {
  return cause.kind === "isoloom" ? t("labs.runOn.refusedHere", { reason: cause.reason }) : t("errors.codes.turnedOffByPolicy", { setting: cause.setting });
}
