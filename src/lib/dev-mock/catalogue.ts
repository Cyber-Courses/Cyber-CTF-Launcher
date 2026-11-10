// The catalogue GraphQL operations in the browser preview (`api_query`): the labs a switch adds
// to the catalogue (`catalogue=arm64vm`), else the sample data's; and where those labs run on the
// scenario's machine (`lab_run_options`).
import { argText } from "@/lib/dev-mock/command";
import { extraLabs, labs, runOptions } from "@/lib/dev-mock/labs";
import type { BrowserHandler } from "@/lib/dev-mock/live";
import { answer } from "@/lib/dev-mock/sample";

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
 *  VMs are built for another CPU than the machine's runs as VMs here only on the emulator. */
export const runOptionsQuery: BrowserHandler = (args, { sc }) => {
  const options = runOptions(args?.runtime);
  const lab = [...labs, ...extraLabs].find((l) => l.id === args?.id);
  const cpus = lab?.runtime.architectures ?? [];
  const emulated = options.vms.length > 0 && cpus.length > 0 && !cpus.includes(sc.arch);
  return emulated ? { ...options, emulated, vms: options.vms.filter((p) => p.provider === "qemu") } : options;
};
