import type { RunTarget } from "@/features/labs/run-on";
import type { LabStatus, ServerHost } from "@/lib/tauri";
import type { T } from "@/lib/i18n";

/** The lab's runtime, as the panels' header meta. */
export const runtimeWord = (t: T, { isDocker, isCloud }: { isDocker: boolean; isCloud: boolean }) =>
  isCloud ? t("labs.detail.cloudServices") : isDocker ? t("labs.detail.containers") : "vm";

/** "This machine", with the engine or hypervisor it runs on when known. */
export const thisMachine = (t: T, engine: string | null) => (engine ? t("labs.detail.thisMachineWith", { engine }) : t("labs.detail.thisMachine"));

/** Where this deploy is headed, shown in the Deployment panel so it's clear during a build, and
 *  where it runs once up: the status says (a container lab in a VM here reads "a VM on this
 *  machine" whatever this page's picker holds, which a reopened app has reset). */
export function deployDestination(t: T, status: LabStatus | undefined, runOn: RunTarget, hosts: ServerHost[], here: Here): string {
  if (status?.host != null) return status.host;
  if (runOn.kind === "host") return hosts.find((h) => h.id === runOn.id)?.name ?? t("labs.detail.yourServer");
  if (runOn.kind === "local-vm" || here.inLocalVm) return t("labs.detail.aVmHere");
  return thisMachine(t, here.engine);
}

/** This machine as the status reports it: the engine or hypervisor, and whether a container lab
 *  runs in a VM here. */
export type Here = { engine: string | null; inLocalVm?: boolean };
