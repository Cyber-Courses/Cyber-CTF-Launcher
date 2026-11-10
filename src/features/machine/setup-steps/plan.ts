import type { Dependency, DockerEngine, Provider, SetupAction, SystemReport } from "@/lib/tauri";

// What the core's plan says each choice of the setup takes on this machine (the report's `setup`,
// Rust's `machine::setup`): the steps render these actions and run each through its command,
// never working out an install, a link or a restart themselves.

/** What makes `engine` the one Docker uses here: nothing when it is. */
export const engineActions = (report: SystemReport, engine: DockerEngine): SetupAction[] =>
  report.setup.engines.find((o) => o.engine === engine)?.actions ?? [];

/** What gets hypervisor `provider` here: nothing once it is installed and can run. */
export const getActions = (report: SystemReport, provider: Provider): SetupAction[] => report.setup.hypervisors.find((o) => o.provider === provider)?.get ?? [];

/** What Vagrant still needs to drive `provider` (Vagrant, its plugin, VMware's utility): nothing once it drives it. */
export const vagrantActions = (report: SystemReport, provider: Provider): SetupAction[] =>
  report.setup.hypervisors.find((o) => o.provider === provider)?.vagrant ?? [];

/** Of `actions`, the one about `dependency`: its install, or the player's to get. */
export const actionFor = (actions: SetupAction[], dependency: Dependency): SetupAction | undefined =>
  actions.find((a) => (a.kind === "install" || a.kind === "manual") && a.dependency === dependency);

/** Why installed hypervisor `provider` can't run here, when nothing the setup installs mends it. */
export function blockedReason(report: SystemReport, provider: Provider): string | null {
  const blocked = getActions(report, provider).find((a) => a.kind === "blocked");
  return blocked?.kind === "blocked" ? blocked.reason : null;
}
