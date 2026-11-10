import type { RunTarget } from "@/features/labs/run-target";
import type { ServerHost } from "@/lib/tauri";
import { placeText, type Place } from "@/features/labs/place";
import type { T } from "@/lib/i18n";

/** The lab's runtime, as the panels' header meta. */
export const runtimeWord = (t: T, { isDocker, isCloud }: { isDocker: boolean; isCloud: boolean }) =>
  isCloud ? t("labs.detail.cloudServices") : isDocker ? t("labs.detail.containers") : t("labs.detail.vms");

/** Where this deploy is headed, shown in the Deployment panel so it's clear during a build, and
 *  where it runs once up, said as the header and Details say it ([`placeText`]): the place the
 *  status records, unless this page's picker heads elsewhere (a server not run on yet, a VM here
 *  for a lab last run as containers). */
export function deployDestination(t: T, place: Place | null, runOn: RunTarget, hosts: ServerHost[]): string {
  if (place?.kind === "server" || place?.kind === "cloud") return placeText(t, place);
  if (runOn.kind === "host") return hosts.find((h) => h.id === runOn.id)?.name ?? t("labs.detail.yourServer");
  if (runOn.kind === "local-vm" && place?.kind !== "hypervisor") return t("labs.detail.aVmHere");
  return place ? placeText(t, place) : t("labs.detail.thisMachine");
}
