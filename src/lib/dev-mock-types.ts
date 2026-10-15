// The dev mock's types (development only, see dev-mock.ts): what each command it answers returns,
// as the typed wrappers in lib/tauri declare it with the types generated from the Rust structs.
// A fixture that drifts from the core (a field renamed, added or made optional) then fails
// `pnpm typecheck` instead of crashing the preview and the end-to-end tests.
import type {
  ActiveOperation,
  AgentInfo,
  AuthStatus,
  CloudPreview,
  ExegolStatus,
  HostCapacity,
  InstalledTool,
  LabCheck,
  LabStatus,
  LabTool,
  MachineMetrics,
  ServerList,
  ServerTest,
  SharedFolder,
  Storage,
  SystemReport,
  Workload,
} from "@/lib/tauri";

export type Answers = {
  auth_status: AuthStatus;
  auth_login: AuthStatus;
  agent_info: AgentInfo;
  system_check: SystemReport;
  machine_metrics: MachineMetrics;
  machine_workloads: Workload[];
  machine_storage: Storage;
  installed_tools: InstalledTool[];
  image_download_size: number | null;
  exegol_status: ExegolStatus;
  attack_vm_status: ExegolStatus;
  shared_folder_get: SharedFolder;
  shared_folder_set: SharedFolder;
  server_list: ServerList;
  server_capacity: HostCapacity | null;
  server_test: ServerTest;
  aws_month_to_date_cost: number | null;
  aws_cli_identity: string | null;
  aws_profiles: string[];
  lab_status: LabStatus;
  lab_cloud_preview: CloudPreview | null;
  running_labs: string[];
  deploying_labs: string[];
  active_operations: ActiveOperation[];
  lab_deploy_log: string;
  lab_tools: LabTool[];
  lab_check: LabCheck;
};

export type Args = Record<string, unknown> | undefined;
export type Handler = (args: Args) => unknown;

/** A table of command handlers: any command, those in `Answers` with the type they return. */
export type Handlers = Record<string, Handler> & { [K in keyof Answers]?: (args: Args) => Answers[K] };
