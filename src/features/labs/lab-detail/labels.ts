import type { RunTarget } from "@/features/labs/run-on";
import type { LabStatus, ServerHost } from "@/lib/tauri";
import type { T } from "@/lib/i18n";

/** The lab's runtime, as the panels' header meta. */
export const runtimeWord = (t: T, { isDocker, isCloud }: { isDocker: boolean; isCloud: boolean }) =>
  isCloud ? t("labs.detail.cloudServices") : isDocker ? t("labs.detail.containers") : "vm";

/** "This machine", with the engine or hypervisor it runs on when known. */
export const thisMachine = (t: T, engine: string | null) => (engine ? t("labs.detail.thisMachineWith", { engine }) : t("labs.detail.thisMachine"));

/** Where this deploy is headed, shown in the Deployment panel so it's clear during a build. */
export function deployDestination(t: T, status: LabStatus | undefined, runOn: RunTarget, hosts: ServerHost[], engine: string | null): string {
  if (status?.host != null) return status.host;
  if (runOn.kind === "host") return hosts.find((h) => h.id === runOn.id)?.name ?? t("labs.detail.yourServer");
  if (runOn.kind === "local-vm") return t("labs.detail.aVmHere");
  return thisMachine(t, engine);
}
