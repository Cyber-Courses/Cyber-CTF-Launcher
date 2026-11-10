import { ENGINES, engineName } from "@/features/machine/setup-steps/engines";
import { providerName } from "@/features/machine/hypervisors";
import { isCloud } from "@/features/servers/host-setup/setup-model";
import { PROVIDER_NAMES, type CloudProvider, type DockerEngine, type LabStatus, type RemoteProvider, type ServerHost } from "@/lib/tauri";
import type { T } from "@/lib/i18n";

// Where a running lab runs, as one fact the page tells three times the same way: the header's
// "on …", Details' "Runs on", and the network diagram's host card with the place's logo.

export type Place =
  /** Containers on this machine: the engine the lab runs on (null: unknown, the machine report not in). */
  | { kind: "engine"; engine: DockerEngine | null }
  /** VMs on this machine, on `provider`; `inVm`: a container lab whose containers run in one VM
   *  here (not one run as one VM per machine, whose VMs are its machines, as a VM lab's are). */
  | { kind: "hypervisor"; provider: string; inVm: boolean }
  /** One of the player's servers (Proxmox, ESXi): the saved host, when it is still saved. */
  | { kind: "server"; name: string; provider: RemoteProvider | null; address: string | null }
  /** One of the player's cloud accounts, in `region`. */
  | { kind: "cloud"; name: string; provider: CloudProvider; region: string };

const isEngine = (id: string): id is DockerEngine => ENGINES.some((e) => e.id === id) || id === "podman";

/** Where the lab runs, from its status: the engine it was started on (recorded at launch) or the
 *  machine's current one, the hypervisor the status names, or the saved host of its id. */
export function placeOf(status: LabStatus | undefined, isDocker: boolean, currentEngine: DockerEngine | null, hosts: ServerHost[]): Place | null {
  if (!status) return null;
  if (status.host !== null) {
    const host = hosts.find((h) => h.id === status.hostId);
    if (host && isCloud(host.provider)) return { kind: "cloud", name: host.name, provider: host.provider, region: host.host };
    return { kind: "server", name: status.host, provider: host?.provider ?? null, address: host?.host ?? null };
  }
  if (isDocker && status.place !== "local_vm") {
    const engine = status.provider && isEngine(status.provider) ? status.provider : currentEngine;
    return { kind: "engine", engine };
  }
  return { kind: "hypervisor", provider: status.provider ?? "", inVm: isDocker && !status.perMachine };
}

/** The place's own name: the engine, the hypervisor, the server, the cloud. */
export function placeName(place: Place): string {
  switch (place.kind) {
    case "engine":
      return place.engine ? engineName(place.engine) : "";
    case "hypervisor":
      return providerName(place.provider);
    case "server":
      return place.name;
    case "cloud":
      return PROVIDER_NAMES[place.provider];
  }
}

/** The place as the header's "on …" and Details' "Runs on" say it: "this machine · Docker
 *  Desktop", "a VM on this machine · QEMU", "homelab · Proxmox", "AWS · eu-west-3". */
export function placeText(t: T, place: Place): string {
  const name = placeName(place);
  switch (place.kind) {
    case "engine":
      return name ? t("labs.detail.thisMachineWith", { engine: name }) : t("labs.detail.thisMachine");
    case "hypervisor":
      if (!name) return place.inVm ? t("labs.detail.aVmHere") : t("labs.detail.thisMachine");
      return place.inVm ? t("labs.detail.aVmHereWith", { hypervisor: name }) : t("labs.detail.thisMachineWith", { engine: name });
    case "server":
      return place.provider ? t("labs.detail.serverWith", { name, kind: PROVIDER_NAMES[place.provider] }) : name;
    case "cloud":
      return t("labs.detail.cloudWith", { cloud: name, region: place.region });
  }
}
