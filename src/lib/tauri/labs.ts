import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";
import type {
  ActiveOperation,
  Badge,
  CloudPreview,
  AttackStatus,
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
  RunTier,
  Runtime,
  SharedFolder,
  TermEvent,
} from "@/lib/tauri/generated";

export type {
  ActiveOperation,
  Badge,
  CloudPreview,
  AttackStatus,
  RunTier,
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

/** The lab's on-demand attack box as the runner reports it, and the lab networks it can join
 *  (`network`: the one the box is on, or null to read its first). Known before the box is up. */
export const attackBoxStatus = (id: string, runtime: Runtime, network: string | null) => invoke<AttackStatus>("attack_box_status", { id, runtime, network });

/** Hot-adds the attack box into the running lab, on `network` (null: the only one), with `image`. */
export const attackBoxAttach = (id: string, runtime: Runtime, network: string | null, image: string, onLog: (line: string) => void) =>
  invokeStreaming("attack_box_attach", { id, runtime, pick: { network, image } }, onLog);

/** Takes the attack box back out of the running lab. */
export const attackBoxDetach = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("attack_box_detach", { id, runtime }, onLog);

/** Opens the attack box shell in the system terminal, on the command the runner builds. */
export const labAttackShell = (id: string, runtime: Runtime) => invokeUnit("lab_attack_shell", { id, runtime });
/** Where and how a lab starts. `defaultPorts`: a container lab here publishes its services on
 *  their own ports, not random ones. `offline`: start the installed lab without Cyber CTF when it
 *  can't be reached (no progress saved), after a launch failed with the `offlineReady` code. */
export type LabLaunch = {
  labId: string;
  provider: Provider | null;
  host: string | null;
  attackboxImage: string | null;
  defaultPorts: boolean;
  offline?: boolean;
};

export const labLaunch = ({ offline = false, ...launch }: LabLaunch, onLog: (line: string) => void) =>
  invokeStreaming("lab_launch", { ...launch, offline }, onLog);

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
export const lingerQuit = () => invokeUnit("linger_quit");

/** Quit the app even though a deploy is in progress (the user confirmed from the warning). */
export const forceQuit = () => invokeUnit("force_quit");

/** Opens (or focuses) the shell window for a lab. */
export const terminalWindow = (id: string, runtime: Runtime, title: string) => invokeUnit("terminal_window", { id, runtime, title });

/** The shell a terminal opens, and its size in cells. */
export type TerminalSpec = { id: string; runtime: Runtime; cols: number; rows: number };

/** Starts the shell in a pseudo-terminal; returns the session that write/resize/close take. */
export const terminalOpen = (spec: TerminalSpec, onEvent: (e: TermEvent) => void) => invokeStreaming<number>("terminal_open", spec, onEvent, "events");
export const terminalWrite = (session: number, data: string) => invokeUnit("terminal_write", { session, data });
export const terminalResize = (session: number, cols: number, rows: number) => invokeUnit("terminal_resize", { session, cols, rows });
export const terminalClose = (session: number) => invokeUnit("terminal_close", { session });

export const sharedFolderGet = () => invoke<SharedFolder>("shared_folder_get");
export const sharedFolderSet = (enabled: boolean, path: string) => invoke<SharedFolder>("shared_folder_set", { enabled, path });
export const sharedFolderOpen = () => invokeUnit("shared_folder_open");

/** How far each launch path has been proven, from Isoloom's tier evidence (for the Beta/Experimental badge). */
export const runTiers = () => invoke<RunTier[]>("run_tiers");
