import type { DockerEngine, Provider } from "@/lib/tauri";
import type { PortMode } from "@/lib/settings";

/** Where a lab starts: this machine (Docker; or, for a lab with VM recipes too, one VM per
 *  machine on a local hypervisor), or a saved host.
 *  `ports` on this machine: chosen in the dialog (container labs), else asked at the start. */
export type RunTarget =
  | { kind: "local"; ports?: PortMode; engine?: DockerEngine; provider?: Provider }
  | { kind: "local-vms"; provider: Provider }
  | { kind: "host"; id: string; perMachine?: boolean };

/** A container lab's VM form here, on a hypervisor: one VM per machine (`local-vms`), its VM
 *  recipes run as a VM lab's. */
export type LocalVmForm = Extract<RunTarget, { kind: "local-vms" }>;

/** Whether `target` is this machine's VM form of a container lab ([`LocalVmForm`]). */
export const isLocalVmForm = (target: RunTarget): target is LocalVmForm => target.kind === "local-vms";
