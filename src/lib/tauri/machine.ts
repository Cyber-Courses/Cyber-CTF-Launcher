import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming } from "@/lib/tauri/stream";
import type {
  CloudClis,
  Dependency,
  InstalledTool,
  MachineMetrics,
  PkgManager,
  Provider,
  ProviderStatus,
  SelfTestEvent,
  SelfTestKind,
  Storage,
  StoredItem,
  SystemReport,
  Tool,
  Workload,
  WslReport,
  WslState,
} from "@/lib/tauri/generated";

export type {
  CloudClis,
  Dependency,
  InstalledTool,
  MachineMetrics,
  PkgManager,
  Provider,
  ProviderStatus,
  SelfTestEvent,
  SelfTestKind,
  Storage,
  StoredItem,
  SystemReport,
  Tool,
  Workload,
  WslReport,
  WslState,
};

/** A Docker-compatible engine (`docker context` names it). */
export type DockerEngine = NonNullable<SystemReport["dockerEngine"]>;

export const systemCheck = () => invoke<SystemReport>("system_check");
/** Points the Docker CLI at another running engine (`docker context use`). */
export const dockerUseEngine = (engine: DockerEngine) => invoke<void>("docker_use_engine", { engine });
/** Starts an installed engine that isn't running and waits until Docker answers. */
export const dockerStartEngine = (engine: DockerEngine) => invoke<void>("docker_start_engine", { engine });

/** Live machine health (CPU/memory/disk/uptime + running containers). */
export const machineMetrics = () => invoke<MachineMetrics>("machine_metrics");

/** Opens the guided "set up this machine" window, optionally at one step (e.g. "docker", "vm"). */
export const machineOpenSetup = (step?: string) => invoke<void>("machine_open_setup", { step: step ?? null });

/** Restarts Windows in a few seconds (to finish turning WSL 2 on); `reason` is the note
 *  Windows shows meanwhile. Windows only. */
export const machineRestart = (reason: string) => invoke<void>("machine_restart", { reason });

/** Assisted one-click install of a lab dependency, streaming the installer output. */
export const installDependency = (dependency: Dependency, onLog: (line: string) => void) => invokeStreaming("install_dependency", { dependency }, onLog);

export const installedTools = () => invoke<InstalledTool[]>("installed_tools");

/** Removes a tool Cyber CTF installed, streaming the output. */
export const uninstallDependency = (dependency: Dependency, onLog: (line: string) => void) => invokeStreaming("uninstall_dependency", { dependency }, onLog);

/** Windows: adds this account to Hyper-V Administrators (one administrator prompt), so Vagrant drives Hyper-V for it after the next sign-in. */
export const hypervJoinAdmins = () => invoke<void>("hyperv_join_admins");

/** Installs a Vagrant plugin (userland), streaming the output. */
export const installVagrantPlugin = (plugin: string, onLog: (line: string) => void) => invokeStreaming("install_vagrant_plugin", { plugin }, onLog);

/** Runs a setup self-test: `docker` boots a throwaway two-container lab, `vm` checks the hypervisor + Vagrant. */
export const machineSelftest = (kind: SelfTestKind, provider: Provider | null, onEvent: (e: SelfTestEvent) => void) =>
  invokeStreaming<SelfTestEvent>("machine_selftest", { kind, provider }, onEvent, "events");

/** Starts a self-test's download (test image / VM box) in the background, so the test itself is quick. */
export const machineSelftestPrefetch = (kind: SelfTestKind, provider: Provider | null) => invoke<void>("machine_selftest_prefetch", { kind, provider });

export const machineWorkloads = () => invoke<Workload[]>("machine_workloads");
export const machineWorkloadStop = (kind: Workload["kind"], id: string) => invoke<void>("machine_workload_stop", { kind, id });

export const machineStorage = (extraImages: string[]) => invoke<Storage>("machine_storage", { extraImages });
/** Removes them (not ones still in use). Resolves to the bytes freed. */
export const machineStorageClean = (extraImages: string[]) => invoke<number>("machine_storage_clean", { extraImages });

/** Compressed download size (bytes) of an image for this machine, from Docker Hub; null if unknown. */
export const imageDownloadSize = (image: string) => invoke<number | null>("image_download_size", { image });
