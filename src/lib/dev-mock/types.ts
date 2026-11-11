// The dev mock's types (development only, see index.ts): what each command it answers returns,
// as the typed wrappers in lib/tauri declare it with the types generated from the Rust structs.
// A fixture that drifts from the core (a field renamed, added or made optional) then fails
// `pnpm typecheck` instead of crashing the preview and the end-to-end tests.
import type {
  ActiveOperation,
  AgentInfo,
  AttackStatus,
  AuthStatus,
  AzureSubscription,
  CloudPreview,
  GcpBillingAccount,
  GcpOrganization,
  HostCapacity,
  ImportedLab,
  LabRemoval,
  InstalledTool,
  LabCheck,
  LabRequirements,
  LabStatus,
  LabTool,
  MachineMetrics,
  OciConfig,
  LabRunOptions,
  LabStartPlace,
  ManagedSettings,
  ServerList,
  ServerTest,
  SharedFolder,
  SignIn,
  Storage,
  SystemReport,
  Workload,
} from "@/lib/tauri";

type Answers = {
  auth_status: AuthStatus;
  auth_login: AuthStatus;
  auth_login_device: AuthStatus;
  agent_info: AgentInfo;
  system_check: SystemReport;
  machine_metrics: MachineMetrics;
  machine_workloads: Workload[];
  machine_storage: Storage;
  installed_tools: InstalledTool[];
  image_download_size: number | null;
  attack_box_status: AttackStatus;
  attack_box_sign_in: SignIn | null;
  shared_folder_get: SharedFolder;
  shared_folder_set: SharedFolder;
  server_list: ServerList;
  server_capacity: HostCapacity | null;
  server_running_labs: Record<string, number>;
  server_test: ServerTest;
  aws_month_to_date_cost: number | null;
  aws_cli_identity: string | null;
  aws_profiles: string[];
  azure_subscriptions: AzureSubscription[];
  gcp_account: string | null;
  gcp_billing_accounts: GcpBillingAccount[];
  gcp_organizations: GcpOrganization[];
  oci_config: OciConfig;
  lab_status: LabStatus;
  lab_cloud_preview: CloudPreview | null;
  lab_requirements: LabRequirements | null;
  lab_default_place: LabStartPlace;
  lab_run_options: LabRunOptions;
  running_labs: string[];
  busy_labs: string[];
  active_operations: ActiveOperation[];
  lab_deploy_log: string;
  lab_tools: LabTool[];
  imported_labs: ImportedLab[];
  lab_installed: boolean;
  lab_remove: LabRemoval;
  lab_check: LabCheck;
  managed_settings: ManagedSettings;
};

export type Args = Record<string, unknown> | undefined;
export type Handler = (args: Args) => unknown;

/** A table of command handlers: any command, those in `Answers` with the type they return. */
export type Handlers = Record<string, Handler> & { [K in keyof Answers]?: (args: Args) => Answers[K] };
