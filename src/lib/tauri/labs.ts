import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";
import type {
  ActiveOperation,
  Badge,
  CloudPreview,
  AttackStatus,
  ImportedLab,
  LabChallenge,
  LabCheck,
  LabCheckResult,
  LabFileProblem,
  LabInterface,
  LabMachine,
  LabNetwork,
  LabOutput,
  LabPlace,
  LabPort,
  LabRequirements,
  LabRunOptions,
  LabRunPath,
  LabRunRefusalCause,
  LabService,
  LabStartPlace,
  LabConsole,
  LabControl,
  LabStatus,
  LabTool,
  Park,
  Provider,
  Runtime,
  SharedFolder,
  ScreenConnection,
  TermEvent,
} from "@/lib/tauri/generated";
import type { DockerEngine } from "@/lib/tauri/machine";

/** Each provider's name as the player reads it, from the backend (`Provider::label`). */
export { EMULATORS, LOCAL_PROVIDERS, PROVIDER_NAMES, PROVIDER_NAMES_ON } from "@/lib/tauri/generated/ProviderNames";

/** The marks a run's closing line starts with, and how the launcher's own deploy-log lines start, by
 *  the step the lab page files them under (`deploy_log`). */
export { DONE_MARK, FAILED_CODE, FAILED_MARK, FETCH_LINES, PREPARE_LINES } from "@/lib/tauri/generated/DeployLog";

export type {
  ActiveOperation,
  Badge,
  CloudPreview,
  AttackStatus,
  ImportedLab,
  LabChallenge,
  LabCheck,
  LabCheckResult,
  LabFileProblem,
  LabInterface,
  LabMachine,
  LabNetwork,
  LabConsole,
  LabControl,
  LabOutput,
  LabPlace,
  LabPort,
  LabRequirements,
  LabRunOptions,
  LabRunPath,
  LabRunRefusalCause,
  LabService,
  LabStartPlace,
  LabStatus,
  LabTool,
  Park,
  Runtime,
  ScreenConnection,
  SharedFolder,
  TermEvent,
};

/** A cloud lab's cost and inputs before it starts; null for other labs. */
export const labCloudPreview = (id: string, repository: string, commit: string) =>
  invoke<CloudPreview | null>("lab_cloud_preview", { id, source: { repository, commit } });

/** Where a lab (declared `runtime`) starts when no place is picked, by the backend's one rule:
 *  where it last ran, else a hypervisor ready here for a VM lab (`preferred`, the Settings choice,
 *  first), else the default server the lab's spec (at the catalogue's pinned commit) runs on,
 *  else here. Also a VM lab's hypervisor should it start here (`hypervisor`). */
export const labDefaultPlace = (id: string, runtime: Runtime, repository: string, commit: string, preferred: Provider | null) =>
  invoke<LabStartPlace>("lab_default_place", { id, runtime, source: { repository, commit }, preferred });

/** Every way a lab (declared `runtime`) can start, from its spec at the catalogue's pinned commit:
 *  per form, the hypervisors, servers and clouds it runs on, each with its launch path's badge,
 *  and where Isoloom refuses it. */
export const labRunOptions = (id: string, runtime: Runtime, repository: string, commit: string) =>
  invoke<LabRunOptions>("lab_run_options", { id, runtime, source: { repository, commit } });

/** A lab's minimum-to-run requirements (CPU/RAM/disk) as a lightest-to-heaviest range; null for a lab with no machines of its own. */
export const labRequirements = (id: string, repository: string, commit: string) =>
  invoke<LabRequirements | null>("lab_requirements", { id, source: { repository, commit } });

export const labStop = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("lab_stop", { id, runtime }, onLog);

/** Pauses or shuts a lab down, keeping its machines; `labResume` brings it back as it was. */
export const labPark = (id: string, runtime: Runtime, mode: Park, onLog: (line: string) => void) => invokeStreaming("lab_park", { id, runtime, mode }, onLog);

/** Runs a VM lab's provisioners again on one machine (or all with `null`), in place. */
export const labProvision = (id: string, runtime: Runtime, machine: string | null, onLog: (line: string) => void) =>
  invokeStreaming("lab_provision", { id, runtime, machine }, onLog);

export const labResume = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("lab_resume", { id, runtime }, onLog);

export const labStatus = (id: string, runtime: Runtime) => invoke<LabStatus>("lab_status", { id, runtime });

/** Lab ids with something up on this machine right now, asked of Docker and each hypervisor
 *  (VMware, VirtualBox, Parallels, UTM, libvirt, Hyper-V): which labs to read the status of again,
 *  never whether one runs (`labStatus` says that). */
export const runningLabs = () => invoke<string[]>("running_labs");

/** The lab's observers, at their addresses where it runs. */
export const labTools = (id: string, runtime: Runtime) => invoke<LabTool[]>("lab_tools", { id, runtime });

/** Runs a lab's exploitability check: is the intended exploit path still solvable? */
export const labCheck = (id: string, runtime: Runtime) => invoke<LabCheck>("lab_check", { id, runtime });

/** The lab's attack box (a machine the launcher adds to the lab) as the runner reports it, and
 *  the lab networks it can join. Known before the box is up. */
export const attackBoxStatus = (id: string, runtime: Runtime) => invoke<AttackStatus>("attack_box_status", { id, runtime });

/** The learner's pick for an attack box: the lab network it joins (null: the seat the lab declares,
 *  else its first network), the image a container box runs (null: the launcher's default), and
 *  whether a VM box (Kali) gets its desktop. */
export type AttackPick = { network: string | null; image: string | null; desktop: boolean };

/** Adds the attack box to the running lab as one of its machines, as `pick` says. */
export const attackBoxAttach = (id: string, runtime: Runtime, pick: AttackPick, onLog: (line: string) => void) =>
  invokeStreaming("attack_box_attach", { id, runtime, pick }, onLog);

/** Takes the attack box back out of the running lab. */
export const attackBoxDetach = (id: string, runtime: Runtime, onLog: (line: string) => void) => invokeStreaming("attack_box_detach", { id, runtime }, onLog);

/** Opens the attack box shell in the system terminal, on the command the runner builds. */
export const labAttackShell = (id: string, runtime: Runtime) => invokeUnit("lab_attack_shell", { id, runtime });
/** Where and how a lab starts. `defaultPorts`: a container lab here publishes its services on
 *  their own ports, not random ones. `offline`: start the installed lab without Cyber CTF when it
 *  can't be reached (no progress saved), after a launch failed with the `offlineReady` code.
 *  `perMachineVm`: a container lab whose spec has VM recipes too runs here as one VM per machine
 *  (the form of a VM lab) on `provider`, instead of as containers. */
export type LabLaunch = {
  labId: string;
  provider: Provider | null;
  host: string | null;
  defaultPorts: boolean;
  offline?: boolean;
  perMachineVm?: boolean;
  /** The container engine a lab here runs on, when the player chose one other than the current
   *  context. Null/omitted: the current context, as before. */
  dockerEngine?: DockerEngine | null;
};

export const labLaunch = ({ offline = false, perMachineVm = false, ...launch }: LabLaunch, onLog: (line: string) => void) =>
  invokeStreaming("lab_launch", { ...launch, offline, perMachineVm }, onLog);

/** Imports and starts a lab from a local directory `path` (an unpublished lab, bypassing the
 *  catalogue and the backend); the deploy's log is streamed to `onLog`. The lab then appears in
 *  the running labs by its derived `local-<name>` id, driven by the ordinary lifecycle commands. */
export const labLaunchLocal = (path: string, onLog: (line: string) => void) => invokeStreaming("lab_launch_local", { path }, onLog);

/** The labs imported from a folder installed here, each with what its own `.ctf/` folder says of
 *  it (title, description, category, difficulty, challenges) and which of those files can't be read. */
export const importedLabs = () => invoke<ImportedLab[]>("imported_labs");

/** The labs with any operation in flight (a start, a stop, a park, a provisioning run), in the
 *  app or a deploy worker: what an update install waits for (Rust's `busy_labs`). */
export const busyLabs = () => invoke<string[]>("busy_labs");

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

/** Opens (or focuses) the screen window of a lab machine (its graphical console). */
export const screenWindow = (id: string, runtime: Runtime, machine: string, title: string) => invokeUnit("screen_window", { id, runtime, machine, title });

/** The screen of the calling screen window's machine: VNC through the window's bridge, or RDP for the
 *  system's client. */
export const screenConnect = () => invoke<ScreenConnection>("screen_connect");

/** Opens the calling screen window's Remote Desktop in the system's client. */
export const screenOpenRdp = () => invokeUnit("screen_open_rdp");

/** The calling screen window's Remote Desktop password, to copy on request. */
export const screenPassword = () => invoke<string>("screen_password");

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
