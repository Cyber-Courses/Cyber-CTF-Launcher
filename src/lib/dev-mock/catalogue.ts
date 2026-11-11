// The catalogue GraphQL operations in the browser preview (`api_query`): the labs a switch adds
// to the catalogue (`catalogue=arm64vm`), else the sample data's; and where those labs run on the
// scenario's machine (`lab_run_options`) and start with nothing picked (`lab_default_place`).
import { argText } from "@/lib/dev-mock/command";
import { hostsOf } from "@/lib/dev-mock/hosts";
import { extraLabs, labs, runOptions, utmPath } from "@/lib/dev-mock/labs";
import type { BrowserHandler } from "@/lib/dev-mock/live";
import { readyHere } from "@/lib/dev-mock/machine";
import { answer } from "@/lib/dev-mock/sample";
import type { LabRunOptions, LabStartPlace, Provider, ServerList } from "@/lib/tauri";
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
  const emulated = options.vms.length > 0 && cpus.length > 0 && !cpus.includes(sc.arch);
  return emulated ? { ...options, emulated, vms: options.vms.filter((p) => p.provider !== null && EMULATORS.includes(p.provider)) } : options;
};

/** Where a lab starts with nothing picked (`lab_default_place`), by the backend's rule
 *  (runtime/start_place.rs) on the scenario's machine: a VM lab on a hypervisor ready here that
 *  runs it (an emulator only, for VMs built for another CPU), the preferred one first, else on the
 *  default server when its spec runs there; any other lab here, on Docker. The preview keeps no
 *  record of where a lab last ran, so that first rule never applies. */
export const defaultPlaceQuery: BrowserHandler = (args, ctx) => {
  const here: LabStartPlace = { host: null, provider: null, perMachineVm: false, hypervisor: null };
  if (args?.runtime !== "VM") return here;
  const options = runOptionsQuery(args, ctx) as LabRunOptions;
  const offers = (paths: LabRunOptions["vms"], p: Provider | null) => paths.some((path) => path.provider === p);
  const ready = readyHere(ctx.sc, ctx.live).filter((p) => offers(options.vms, p));
  const preferred = args.preferred as Provider | null | undefined;
  const hypervisor = (preferred && ready.includes(preferred) ? preferred : ready.at(0)) ?? null;
  if (hypervisor) return { ...here, provider: hypervisor, hypervisor };
  const id = (answer("server_list", undefined) as ServerList).default;
  const server = hostsOf(ctx.sc).find((h) => h.id === id);
  return server && offers(options.servers, server.provider as Provider) ? { ...here, host: id } : here;
};
