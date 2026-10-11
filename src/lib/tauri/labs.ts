import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming } from "@/lib/tauri/stream";
import type { Provider } from "@/lib/tauri/machine";

/** How a lab runs: containers, VMs, or cloud services (a Terraform module of its own, applied
 *  into the player's cloud account). */
export type Runtime = "DOCKER" | "VM" | "CLOUD";

/** A machine's address on one lab network (the lab's name for it, e.g. "dmz"). */
export interface LabInterface {
  network: string;
  ip: string;
}

export interface LabMachine {
  name: string;
  state: string;
  image: string;
  ip: string;
  ports: { published: number; target: number }[];
  /** Every network it is plugged into; two or more = a pivot. Empty when unknown. */
  interfaces: LabInterface[];
  /** Services the lab declares inside it (compose labels); empty when none are declared. */
  services: LabService[];
  /** Lab plumbing (the provisioning controller), not a target: hidden, powered off once built. */
  infra: boolean;
}

/** A service inside a machine, as the lab declares it (`cyberctf.service.<name>`). */
export interface LabService {
  name: string;
  /** As declared: "web", "database", "cache", "worker", "ssh" or free text. */
  kind: string;
  /** Ports it listens on inside the container. */
  ports: number[];
}

/** A lab network segment (a Docker network = a switch). */
export interface LabNetwork {
  name: string;
  subnet: string;
  /** No route out (compose `internal: true`). */
  internal: boolean;
}

/** How a lab was parked: paused (state saved, resumes in seconds) or shut down (powered off). */
export type Park = "pause" | "shutdown";

export interface LabStatus {
  running: boolean;
  /** Set when the launcher parked the lab and it is still down: its machines resume as they
   *  were, so the page offers Resume rather than "clean up". */
  parked: Park | null;
  machines: LabMachine[];
  /** The lab's network segments (Docker labs); empty when the runtime doesn't report them. */
  networks: LabNetwork[];
  /** Loopback URL where the lab is reachable, once running (null for VM labs / no port). */
  url: string | null;
  /** Server host name a VM lab runs on; null when it runs on this machine. */
  host: string | null;
  /** Unix seconds a cloud lab stops itself (auto-stop); null if it doesn't. */
  expiresAt: number | null;
  /** Where it runs: its container here, a VM here, a server or a cloud account. */
  place: "container" | "local_vm" | "server" | "cloud" | null;
  /** The engine or hypervisor it runs on: "docker", a Vagrant provider id ("virtualbox",
   *  "vmware_desktop", "parallels", ...) or a server/cloud provider; null when unknown. */
  provider: string | null;
  /** Server and cloud labs: the attack box beside the lab on its host (address and lab
   *  network); null elsewhere (the local attack box reports itself). */
  attacker: { ip: string; labNetwork: string } | null;
  /** What a cloud lab exposes once up (its module's outputs): entry points, credentials. Empty
   *  for the other labs. */
  outputs: LabOutput[];
  /** A cloud lab's own words once up, its outputs filled in; null for the other labs. */
  message: string | null;
}

/** One thing a cloud lab exposes: a module output under the lab's name for it. */
export interface LabOutput {
  name: string;
  value: string;
  /** A secret (Terraform marks it sensitive): masked until the player asks. */
  sensitive: boolean;
}

/** A cloud lab before it starts, from its spec at the catalogue's commit. */
export interface CloudPreview {
  /** "aws", "azure" or "gcp". */
  provider: string;
  /** The lab's own estimate, in US dollars per hour; null when it gives none. */
  hourlyUsd: number | null;
  /** The launch-time inputs it takes (PLAYER_CIDR...). */
  inputs: string[];
  /** It lets in this machine's public IP only. */
  allowList: boolean;
  /** The names of what it exposes once up. */
  outputs: string[];
}

/** A cloud lab's cost and inputs before it starts; null for other labs. */
export const labCloudPreview = (id: string, repository: string, commit: string) => invoke<CloudPreview | null>("lab_cloud_preview", { id, repository, commit });

export const labStop = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("lab_stop", { id, runtime }, onLog);

/** Pauses or shuts a lab down, keeping its machines; `labResume` brings it back as it was. */
export const labPark = (id: string, runtime: Runtime, mode: Park, onLog: (line: string) => void) => invokeStreaming("lab_park", { id, runtime, mode }, onLog);

/** Runs a VM lab's provisioners again on one machine (or all with `null`), in place. */
export const labProvision = (id: string, runtime: Runtime, machine: string | null, onLog: (line: string) => void) =>
  invokeStreaming("lab_provision", { id, runtime, machine }, onLog);

export const labResume = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("lab_resume", { id, runtime }, onLog);

export const labStatus = (id: string, runtime: Runtime) => invoke<LabStatus>("lab_status", { id, runtime });

/** Lab ids running on this machine right now, found by scanning Docker and Vagrant directly.
 *  Recovers running labs after a crash/restart, independent of any per-lab status probe. */
export const runningLabs = () => invoke<string[]>("running_labs");

/** One check of a lab's self-verification. */
export interface LabCheckResult {
  name: string;
  /** Where it ran: "from web", or "from the environment's networks". */
  from: string;
  ok: boolean;
  /** Why it failed (empty when it passed). */
  reason: string;
}

/** Result of a lab's self-verification: its own checks and the ones derived from its spec. */
export interface LabCheck {
  available: boolean;
  ok: boolean;
  output: string;
  results: LabCheckResult[];
}

/** An observer the lab puts beside itself (`tools:`): a toolbox or a capture box on every network, outside the lab's contract. */
export interface LabTool {
  name: string;
  image: string | null;
  /** Its address on each lab network, where the lab runs. */
  addresses: { network: string; ip: string }[];
  /** The loopback port its web UI is published on, when it has one. */
  publish: number | null;
}

/** The lab's observers, at their addresses where it runs. */
export const labTools = (id: string, runtime: Runtime) => invoke<LabTool[]>("lab_tools", { id, runtime });

/** Runs a lab's exploitability check: is the intended exploit path still solvable? */
export const labCheck = (id: string, runtime: Runtime) => invoke<LabCheck>("lab_check", { id, runtime });

export interface ExegolStatus {
  imagePresent: boolean;
  running: boolean;
  ip: string;
  /** The lab network the attack box is plugged into, once running. */
  labNetwork: string;
  shellCmd: string;
}

/** Status of a lab's attack box (Exegol) for the configured image. */
export const exegolStatus = (id: string, image: string) => invoke<ExegolStatus>("exegol_status", { id, image });

export const exegolStart = (id: string, image: string, onLog: (line: string) => void) => invokeStreaming("exegol_start", { id, image }, onLog);

export const exegolStop = (id: string, onLog: (line: string) => void) => invokeStreaming("exegol_stop", { id }, onLog);

/** The attack VM beside a VM lab (a Vagrant box), in the container attack box's status shape. */
export const attackVmStatus = (id: string, boxName: string) => invoke<ExegolStatus>("attack_vm_status", { id, boxName });
export const attackVmStart = (id: string, boxName: string, onLog: (line: string) => void) => invokeStreaming("attack_vm_start", { id, boxName }, onLog);
export const attackVmStop = (id: string, onLog: (line: string) => void) => invokeStreaming("attack_vm_stop", { id }, onLog);
export const attackVmShell = (id: string) => invoke<void>("attack_vm_shell", { id });

/** Opens the attack box shell wherever the lab runs (local container, or SSH to a remote lab host). */
export const labAttackShell = (id: string, runtime: Runtime) => invoke<void>("lab_attack_shell", { id, runtime });
/** `defaultPorts`: a container lab here publishes its services on their own ports, not random ones. */
export const labLaunch = (
  labId: string,
  provider: Provider | null,
  host: string | null,
  attackboxImage: string | null,
  defaultPorts: boolean,
  onLog: (line: string) => void,
) => invokeStreaming("lab_launch", { labId, provider, host, attackboxImage, defaultPorts }, onLog);

/** The lab ids currently starting or stopping in the backend. Read on load to rehydrate the
 *  "starting" state after a window reload, which loses the in-memory deploy store. */
export const deployingLabs = () => invoke<string[]>("deploying_labs");

/** One operation in flight on a lab, with the step its log is at (sidebar). */
export interface ActiveOperation {
  labId: string;
  op: "launch" | "resume" | "pause" | "shutdown" | "provision" | "attack_vm" | "stop";
  machine: string | null;
  step: string | null;
}

/** Every lab operation in flight right now: workers and in-process ones. */
export const activeOperations = () => invoke<ActiveOperation[]>("active_operations");

/** The log so far of a lab's latest deploy, which runs in a detached worker process. Lets a
 *  reloaded or relaunched app re-attach to a deploy still in progress. */
export const labDeployLog = (id: string) => invoke<string>("lab_deploy_log", { id });

/** Quit by finishing in the background: hides the windows and exits once the in-app deploys
 *  (the fallback when no worker could be started) are done. */
export const lingerQuit = () => invoke<void>("linger_quit");

/** Quit the app even though a deploy is in progress (the user confirmed from the warning). */
export const forceQuit = () => invoke<void>("force_quit");

/** Which shell an embedded terminal attaches to: the lab's attack box, or a VM lab's attack VM. */
export type ShellKind = "lab" | "attackVm";
export type TermEvent = { kind: "data"; data: string } | { kind: "exit"; code: number | null };

/** Opens (or focuses) the shell window for a lab. */
export const terminalWindow = (id: string, kind: ShellKind, runtime: Runtime, title: string) => invoke<void>("terminal_window", { id, kind, runtime, title });

/** Starts the shell in a pseudo-terminal; returns the session that write/resize/close take. */
export const terminalOpen = (id: string, kind: ShellKind, runtime: Runtime, cols: number, rows: number, onEvent: (e: TermEvent) => void) =>
  invokeStreaming<TermEvent, number>("terminal_open", { id, kind, runtime, cols, rows }, onEvent, "events");
export const terminalWrite = (session: number, data: string) => invoke<void>("terminal_write", { session, data });
export const terminalResize = (session: number, cols: number, rows: number) => invoke<void>("terminal_resize", { session, cols, rows });
export const terminalClose = (session: number) => invoke<void>("terminal_close", { session });

/** The folder shared with the attack box, at `mountPoint` inside it (off until chosen). */
export type SharedFolder = { enabled: boolean; path: string; mountPoint: string };
export const sharedFolderGet = () => invoke<SharedFolder>("shared_folder_get");
export const sharedFolderSet = (enabled: boolean, path: string) => invoke<SharedFolder>("shared_folder_set", { enabled, path });
export const sharedFolderOpen = () => invoke<void>("shared_folder_open");
