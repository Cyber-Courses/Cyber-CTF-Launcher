import type { DockerEngine, Provider } from "@/lib/tauri";
import type { PortMode, SavedVmForm } from "@/lib/settings";

/** Where a lab starts: this machine (Docker; a container lab in one VM on a local hypervisor;
 *  or, for a lab with VM recipes too, one VM per machine there), or a saved host.
 *  `ports` on this machine: chosen in the dialog (container labs), else asked at the start. */
export type RunTarget =
  | { kind: "local"; ports?: PortMode; engine?: DockerEngine; provider?: Provider }
  | { kind: "local-vm"; provider: Provider }
  | { kind: "local-vms"; provider: Provider }
  | { kind: "host"; id: string; perMachine?: boolean };

/** A container lab's VM form here, on a hypervisor: one helper VM (`local-vm`) or one VM per
 *  machine (`local-vms`). */
export type LocalVmForm = Extract<RunTarget, { kind: "local-vm" | "local-vms" }>;

/** Whether `target` is one of this machine's VM forms ([`LocalVmForm`]). */
export const isLocalVmForm = (target: RunTarget): target is LocalVmForm => target.kind === "local-vm" || target.kind === "local-vms";

/** A VM form's kind as Settings saves it ([`SavedVmForm`]), and back: the one mapping between them. */
export const toSavedForm = (kind: LocalVmForm["kind"]): SavedVmForm => (kind === "local-vm" ? "vm" : "vms");
export const fromSavedForm = (saved: SavedVmForm): LocalVmForm["kind"] => (saved === "vm" ? "local-vm" : "local-vms");
