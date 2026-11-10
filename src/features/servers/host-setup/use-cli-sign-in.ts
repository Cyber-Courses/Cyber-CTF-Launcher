"use client";

import { useCallback, useState } from "react";
import { cloudLoginCancel } from "@/lib/tauri";
import { useRunLog } from "@/lib/use-run-log";
import { errorCode } from "@/lib/errors";
import { ignore } from "@/lib/failure";
import { translate } from "@/lib/i18n";

/** Where a cloud CLI's browser sign-in stands: none yet, waiting for the browser, ended (its
 *  log says how), cancelled by the player, or failed. */
export type SignInState = "idle" | "running" | "done" | "cancelled" | "failed";

/**
 * A cloud CLI's sign-in (`aws login`, `az login`, `gcloud auth login`, …), the same for every
 * cloud: its streamed log, and how it ended. A cancel isn't a failure: it ends `cancelled`, with
 * nothing added to the log (the step hides it and offers Retry); a failure ends on its `✗` line.
 */
export function useCliSignIn() {
  const { lines, run: runLog } = useRunLog();
  const [state, setState] = useState<SignInState>("idle");

  /** Runs `task`, a sign-in to `cloud`, its lines in the log. Never rejects. */
  const run = useCallback(
    async (cloud: string, task: (onLog: (l: string) => void) => Promise<string | undefined>) => {
      setState("running");
      let ended: SignInState = "done";
      await runLog(translate("servers.setup.credentials.logSigningIn", { cloud }), async (onLog) => {
        try {
          return await task(onLog);
        } catch (e) {
          if (errorCode(e) === "signInCancelled") {
            ended = "cancelled";
            return undefined;
          }
          ended = "failed";
          throw e;
        }
      });
      setState(ended);
    },
    [runLog],
  );

  /** Stops the sign-in in progress: its CLI is stopped, and it ends `cancelled`. */
  const cancel = useCallback(() => {
    cloudLoginCancel().catch(ignore("the sign-in ends on its own within its time limit"));
  }, []);

  return { state, lines, running: state === "running", run, cancel };
}
