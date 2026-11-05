// The catalogue GraphQL operations in the browser preview (`api_query`): the labs a switch adds
// to the catalogue (`catalogue=arm64vm`), else the sample data's.
import { argText } from "@/lib/dev-mock/command";
import { extraLabs, labs } from "@/lib/dev-mock/labs";
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
