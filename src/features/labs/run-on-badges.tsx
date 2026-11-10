"use client";

import { isCloudProvider } from "@/lib/tauri";
import type { LabRunOptions, LabRunPath, LabRunRefusalCause, Provider } from "@/lib/tauri";
import type { RunTarget } from "@/features/labs/run-on";
import type { T } from "@/lib/i18n";

// The maturity badge (Beta / Experimental) on each place of "Where should it run?": the badge of
// the launch path the backend says the lab takes there (`labRunOptions`), and why it is refused
// where Isoloom refuses it or this computer's policy turns it off.

/** A maturity badge on a launch path (Beta / Experimental), with the evidence as its tooltip. */
export type RunBadge = { kind: "beta" | "experimental"; title: string };

/** `path`'s badge; none for a proven path, or none known. */
export const badgeOf = (path: LabRunPath | undefined): RunBadge | undefined =>
  path && path.badge !== "none" ? { kind: path.badge, title: path.evidence } : undefined;

const on = (paths: LabRunPath[], provider: Provider | undefined) => paths.find((p) => p.provider === provider);

/** The launch path `target` takes, as the lab's `options` say (the host's `provider` for a saved
 *  server or account): a helper VM's or one VM per machine's on its hypervisor, a server's by its
 *  kind, a cloud account's in the form picked (its default: cloud services, else one VM with the
 *  containers, else one VM per machine). None while the options load, or here on Docker or a VM
 *  lab's own hypervisor (picked later). */
export function pathOf(options: LabRunOptions | null, target: RunTarget, provider?: Provider): LabRunPath | undefined {
  if (!options) return undefined;
  switch (target.kind) {
    case "local":
      return undefined;
    case "local-vm":
      return on(options.dockerVm, target.provider);
    case "local-vms":
      return on(options.vms, target.provider);
    case "host":
      if (!provider || !isCloudProvider(provider)) return on(options.servers, provider);
      if (target.perMachine) return on(options.cloudVm, provider);
      return on([...options.cloudServices, ...options.cloudDocker, ...options.cloudVm], provider);
  }
}

/** Why the lab is refused on `provider` (none: as containers on this machine's Docker), if it is:
 *  Isoloom's reason, or the policy setting that turns it off. */
export function refusalOn(t: T, options: LabRunOptions | null, provider: Provider | null): string | undefined {
  const refusal = options?.refused.find((r) => r.provider === provider);
  return refusal && said(t, refusal.cause);
}

/** A refusal's `cause` in the player's words, naming who refuses: Isoloom, or the organization's
 *  policy. */
function said(t: T, cause: LabRunRefusalCause): string {
  return cause.kind === "isoloom" ? t("labs.runOn.refusedHere", { reason: cause.reason }) : t("errors.codes.turnedOffByPolicy", { setting: cause.setting });
}
