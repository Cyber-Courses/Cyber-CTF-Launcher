import type { ServerTest } from "@/lib/tauri";

/** A connection test's outcome, for its mark: failed on any failure, a warning when a check only
 *  warrants one, else ok. The Cloud page's account rows and the setup's Connection test both
 *  read it here, so a mark never disagrees with the result it marks. */
export type Outcome = "ok" | "warn" | "fail";

export function outcomeOf(result: ServerTest): Outcome {
  if (!result.ok) return "fail";
  return result.checks.some((c) => c.state === "warn") ? "warn" : "ok";
}
