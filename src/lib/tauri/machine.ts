import { invoke } from "@tauri-apps/api/core";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";
import type {
  Dependency,
  DockerEngine,
  InstalledTool,
  MachineMetrics,
  OvftoolPlaced,
  Provider,
  SelfTestEvent,
  SelfTestKind,
  Storage,
  SystemReport,
  WindowsFeature,
  Workload,
} from "@/lib/tauri/generated";

export type {
  CloudClis,
  Dependency,
  DockerEngine,
  EngineOption,
  EngineStatus,
  HypervisorOption,
  InstalledTool,
  MachineMetrics,
  ManualReason,
  Need,
  OvftoolLocation,
  OvftoolPlaced,
  OvftoolRefusal,
  PkgManager,
  Provider,
  ProviderStatus,
  SelfTestEvent,
  SelfTestKind,
  SetupAction,
  SetupOptions,
  Storage,
  StoredItem,
  SystemReport,
  Tool,
  UtilityState,
  WindowsFeature,
  Workload,
  WslReport,
  WslState,
} from "@/lib/tauri/generated";

/** Every Docker-compatible engine the backend names, and each one's name (Rust's `Engine`). */
export { DOCKER_ENGINES, DOCKER_ENGINE_NAMES, DOCKER_ENGINE_URLS } from "@/lib/tauri/generated/DockerEngines";

export const systemCheck = () => invoke<SystemReport>("system_check");
/** Points the Docker CLI at another running engine (`docker context use`). */
export const dockerUseEngine = (engine: DockerEngine) => invokeUnit("docker_use_engine", { engine });
/** Starts an installed engine that isn't running and waits until Docker answers. */
export const dockerStartEngine = (engine: DockerEngine) => invokeUnit("docker_start_engine", { engine });

/** Live machine health (CPU/memory/disk/uptime + running containers). */
export const machineMetrics = () => invoke<MachineMetrics>("machine_metrics");

/** Opens the guided "set up this machine" window, optionally at one step (e.g. "docker", "vm"),
 *  for one hypervisor (a Machine page row's Fix: that one is set up, not another). */
export const machineOpenSetup = (step?: string, provider?: Provider) => invokeUnit("machine_open_setup", { step: step ?? null, provider: provider ?? null });

/** Restarts Windows in a few seconds, to finish turning `feature` on (WSL 2, Hyper-V); `reason` is
 *  the note Windows shows meanwhile. Windows only. */
export const machineRestart = (reason: string, feature: WindowsFeature) => invokeUnit("machine_restart", { reason, feature });

/** Assisted one-click install of a lab dependency, streaming the installer output. */
export const installDependency = (dependency: Dependency, onLog: (line: string) => void) => invokeStreaming("install_dependency", { dependency }, onLog);

export const installedTools = () => invoke<InstalledTool[]>("installed_tools");

/** Copies the OVF Tool unpacked at `folder` into the app's own tools folder, once it is checked
 *  as VMware's (on macOS, its signature), or says why it wasn't. */
export const ovftoolPlace = (folder: string) => invoke<OvftoolPlaced>("ovftool_place", { folder });

/** Removes a tool Cyber CTF installed, streaming the output. */
export const uninstallDependency = (dependency: Dependency, onLog: (line: string) => void) => invokeStreaming("uninstall_dependency", { dependency }, onLog);

/** Windows: adds this account to Hyper-V Administrators (one administrator prompt), so Vagrant drives Hyper-V for it after the next sign-in. */
export const hypervJoinAdmins = () => invokeUnit("hyperv_join_admins");

/** Installs a Vagrant plugin (userland), streaming the output. */
export const installVagrantPlugin = (plugin: string, onLog: (line: string) => void) => invokeStreaming("install_vagrant_plugin", { plugin }, onLog);

/** Runs a setup self-test: `docker` boots a throwaway two-container lab on the Docker CLI
 *  `context` (one engine among several; null for the current one), `vm` a throwaway VM on
 *  `provider` through Vagrant. */
export const machineSelftest = (kind: SelfTestKind, provider: Provider | null, onEvent: (e: SelfTestEvent) => void, context: string | null = null) =>
  invokeStreaming("machine_selftest", { kind, provider, context }, onEvent, "events");

/** Starts a self-test's download (test image / VM box) in the background, so the test itself is quick. */
export const machineSelftestPrefetch = (kind: SelfTestKind, provider: Provider | null) => invokeUnit("machine_selftest_prefetch", { kind, provider });

export const machineWorkloads = () => invoke<Workload[]>("machine_workloads");
export const machineWorkloadStop = (kind: Workload["kind"], id: string) => invokeUnit("machine_workload_stop", { kind, id });

export const machineStorage = (extraImages: string[]) => invoke<Storage>("machine_storage", { extraImages });
/** Removes them (not ones still in use). Resolves to the bytes freed. */
export const machineStorageClean = (extraImages: string[]) => invoke<number>("machine_storage_clean", { extraImages });

/** Compressed download size (bytes) of an image for this machine, from Docker Hub; null if unknown. */
export const imageDownloadSize = (image: string) => invoke<number | null>("image_download_size", { image });
