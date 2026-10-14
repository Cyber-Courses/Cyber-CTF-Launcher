import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming } from "@/lib/tauri/stream";
import type {
  ActiveOperation,
  CloudPreview,
  ExegolStatus,
  LabAttacker,
  LabCheck,
  LabCheckResult,
  LabInterface,
  LabMachine,
  LabNetwork,
  LabOutput,
  LabPlace,
  LabPort,
  LabService,
  LabStatus,
  LabTool,
  Park,
  Provider,
  Runtime,
  SharedFolder,
  ShellKind,
  TermEvent,
} from "@/lib/tauri/generated";

export type {
  ActiveOperation,
  CloudPreview,
  ExegolStatus,
  LabAttacker,
  LabCheck,
  LabCheckResult,
  LabInterface,
  LabMachine,
  LabNetwork,
  LabOutput,
  LabPlace,
  LabPort,
  LabService,
  LabStatus,
  LabTool,
  Park,
  Runtime,
  SharedFolder,
  ShellKind,
  TermEvent,
};

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

/** The lab's observers, at their addresses where it runs. */
export const labTools = (id: string, runtime: Runtime) => invoke<LabTool[]>("lab_tools", { id, runtime });

/** Runs a lab's exploitability check: is the intended exploit path still solvable? */
export const labCheck = (id: string, runtime: Runtime) => invoke<LabCheck>("lab_check", { id, runtime });

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
/** `defaultPorts`: a container lab here publishes its services on their own ports, not random ones.
 *  `offline`: start the installed lab without Cyber CTF when it can't be reached (no progress
 *  saved), after a launch failed saying the lab "can start offline". */
export const labLaunch = (
  labId: string,
  provider: Provider | null,
  host: string | null,
  attackboxImage: string | null,
  defaultPorts: boolean,
  onLog: (line: string) => void,
  offline = false,
) => invokeStreaming("lab_launch", { labId, provider, host, attackboxImage, defaultPorts, offline }, onLog);

/** The lab ids currently starting or stopping in the backend. Read on load to rehydrate the
 *  "starting" state after a window reload, which loses the in-memory deploy store. */
export const deployingLabs = () => invoke<string[]>("deploying_labs");

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

/** Opens (or focuses) the shell window for a lab. */
export const terminalWindow = (id: string, kind: ShellKind, runtime: Runtime, title: string) => invoke<void>("terminal_window", { id, kind, runtime, title });

/** Starts the shell in a pseudo-terminal; returns the session that write/resize/close take. */
export const terminalOpen = (id: string, kind: ShellKind, runtime: Runtime, cols: number, rows: number, onEvent: (e: TermEvent) => void) =>
  invokeStreaming<TermEvent, number>("terminal_open", { id, kind, runtime, cols, rows }, onEvent, "events");
export const terminalWrite = (session: number, data: string) => invoke<void>("terminal_write", { session, data });
export const terminalResize = (session: number, cols: number, rows: number) => invoke<void>("terminal_resize", { session, cols, rows });
export const terminalClose = (session: number) => invoke<void>("terminal_close", { session });

export const sharedFolderGet = () => invoke<SharedFolder>("shared_folder_get");
export const sharedFolderSet = (enabled: boolean, path: string) => invoke<SharedFolder>("shared_folder_set", { enabled, path });
export const sharedFolderOpen = () => invoke<void>("shared_folder_open");

/** Whether the labs started here are recorded in Isoloom's registry (`~/.isoloom/status.yml`). */
export const isoloomRegistryGet = () => invoke<boolean>("isoloom_registry_get");
export const isoloomRegistrySet = (enabled: boolean) => invoke<boolean>("isoloom_registry_set", { enabled });
