// The catalogue GraphQL operations in the browser preview (`api_query`): the labs a switch adds
// to the catalogue (`catalogue=arm64vm`), else the sample data's; and where those labs run on the
// scenario's machine (`lab_run_options`) and where a launch places them (`lab_placement`).
import { argText, fail } from "@/lib/dev-mock/command";
import { hostsOf } from "@/lib/dev-mock/hosts";
import { extraLabs, labs, runOptions, utmPath } from "@/lib/dev-mock/labs";
import type { BrowserHandler } from "@/lib/dev-mock/live";
import { readyHere } from "@/lib/dev-mock/machine";
import { answer } from "@/lib/dev-mock/sample";
import { isCloudProvider, type LabPlacement, type LabPlacementChoice, type LabRunOptions, type Provider, type ServerList } from "@/lib/tauri";
import { EMULATORS } from "@/lib/tauri/generated/ProviderNames";

/** The GraphQL operations that list the catalogue (sample.ts `apiQuery`). */
const CATALOGUE_OPS = ["labs", "labTitles", "sidebarLabs"];

/** A GraphQL operation (`api_query`): the catalogue with the labs a switch adds to it
 *  (`catalogue=arm64vm`), else the sample data's. */
export const catalogueQuery: BrowserHandler = (args, { sc }) => {
  const op = argText(args?.operation);
  if (sc.catalogue === "arm64vm" && CATALOGUE_OPS.includes(op)) return { labs: [...labs, ...extraLabs] };
  return answer("api_query", args);
};

/** Where a lab runs (`lab_run_options`), on the scenario's machine: as the backend says, a lab whose
 *  VMs are built for another CPU than the machine's runs as VMs here only on the emulator. A VM
 *  lab whose catalogue lists UTM (Edge router) has its VMs on UTM too; Isoloom refuses the others
 *  there. */
export const runOptionsQuery: BrowserHandler = (args, { sc }) => {
  const lab = [...labs, ...extraLabs].find((l) => l.id === args?.id);
  const generic = runOptions(args?.runtime);
  const onUtm = generic.vms.length > 0 && !!lab?.runtime.providers.includes("utm");
  const options = onUtm ? { ...generic, vms: [...generic.vms, utmPath], refused: generic.refused.filter((r) => r.provider !== "utm") } : generic;
  const cpus = lab?.runtime.architectures ?? [];
  // A lab names its CPUs as images do (`arm64`), the machine as Rust does (`aarch64`).
  const machineCpu = sc.arch === "aarch64" ? "arm64" : sc.arch;
  const emulated = options.vms.length > 0 && cpus.length > 0 && !cpus.includes(machineCpu);
  return emulated ? { ...options, emulated, vms: options.vms.filter((p) => p.provider !== null && EMULATORS.includes(p.provider)) } : options;
};

/** Where a lab runs for the player's choice (`lab_placement`), by the backend's rule
 *  (runtime/placement.rs) on the scenario's machine: a host picked runs it there; a hypervisor
 *  picked runs its VMs here; a VM lab (or one VM per machine) on a hypervisor ready here that
 *  runs it (an emulator only, for VMs built for another CPU), else on the default server when its
 *  spec runs there; any other lab here, on Docker. */
export const placementQuery: BrowserHandler = (args, ctx): LabPlacement => {
  const choice = (args?.choice ?? {}) as Partial<LabPlacementChoice>;
  const runtime = args?.runtime;
  const options = runOptionsQuery(args, ctx) as LabRunOptions;
  const offers = (paths: LabRunOptions["vms"], p: Provider | null) => paths.some((path) => path.provider === p);
  if (choice.host) {
    const id = choice.host;
    const host = hostsOf(ctx.sc).find((h) => h.id === id);
    if (!host) return fail("that host is no longer set up in the launcher");
    if (isCloudProvider(argText(host.provider)))
      return { kind: "cloud", account: id, perMachine: runtime === "VM" || (runtime === "DOCKER" && !!choice.perMachineVm) };
    return { kind: "server", host: id };
  }
  if (runtime === "CLOUD") return fail("This lab runs in your cloud account: pick one of your cloud accounts to start it on.", "pickCloudAccount");
  if (choice.provider) return { kind: "vms", provider: choice.provider };
  if (runtime === "VM" || choice.perMachineVm) {
    const ready = readyHere(ctx.sc, ctx.live).filter((p) => offers(options.vms, p));
    const provider = ready.at(0);
    if (provider) return { kind: "vms", provider };
    const id = (answer("server_list", undefined) as ServerList).default;
    const server = hostsOf(ctx.sc).find((h) => h.id === id);
    if (runtime === "VM" && !choice.here && id && server && offers(options.servers, server.provider as Provider)) return { kind: "server", host: id };
    return fail("No hypervisor ready on this machine runs this lab.", "noHypervisor");
  }
  return { kind: "docker", engine: choice.engine ?? null, defaultPorts: choice.defaultPorts ?? false };
};
